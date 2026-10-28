//! Authenticated GitHub release discovery with advisory caches and cooldowns.

use std::collections::BTreeMap;
use std::io::Read as _;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use anyhow::{Context as _, Result, bail, ensure};
use base64::Engine as _;
use serde::{Deserialize, Serialize};
use sha2::{Digest as _, Sha256};

use super::env::Env;
use super::exec::{CommandSpec, Executor, OutputLog};

const MAX_METADATA_BYTES: usize = 1024 * 1024;
const CACHE_MAX_AGE: u64 = 600;

/// Keep unattended GitHub commands out of terminal and desktop prompts.
pub(crate) fn unattended_gh() -> CommandSpec {
    CommandSpec::new("gh")
        .env("GH_PROMPT_DISABLED", "1")
        .env("DBUS_SESSION_BUS_ADDRESS", "disabled:")
}

/// Prefer explicit credentials, then probe the active github.com CLI login.
/// Credential output must never be retained in diagnostics.
pub(crate) fn github_token(env: &dyn Env, executor: &dyn Executor) -> Option<String> {
    if let Some(token) = ["GH_TOKEN", "GITHUB_TOKEN"]
        .into_iter()
        .find_map(|key| env.var(key).filter(|value| !value.trim().is_empty()))
    {
        return Some(token);
    }
    if !executor.which("gh") {
        return None;
    }
    let request = unattended_gh()
        .args(&["auth", "token", "--hostname", "github.com"])
        .unchecked()
        .output_log(OutputLog::Omit)
        .timeout(Duration::from_secs(10));
    match executor.execute(request) {
        Ok(result) if result.success && !result.stdout.trim().is_empty() => {
            Some(result.stdout.trim().to_owned())
        }
        Ok(_) | Err(_) => {
            // Optional credential probes can fail or contain secrets even in errors.
            tracing::debug!("GitHub CLI has no usable token; checking releases anonymously");
            None
        }
    }
}

/// Response data needed for conditional requests and rate-limit handling.
#[derive(Debug, Default)]
pub(crate) struct Response {
    pub status: u16,
    pub headers: BTreeMap<String, String>,
    pub body: String,
}

/// Injectable, bounded release-metadata transport.
pub(crate) trait HttpClient: std::fmt::Debug + Send + Sync {
    fn get(&self, url: &str, headers: &[(&str, &str)]) -> Result<Response>;
}

/// Native HTTP transport; authorization never passes through a subprocess.
#[derive(Debug)]
pub(crate) struct UreqClient {
    agent: ureq::Agent,
}

impl UreqClient {
    pub(crate) fn new(timeout: Duration) -> Self {
        Self {
            agent: ureq::config::Config::builder()
                .timeout_global(Some(timeout))
                .build()
                .new_agent(),
        }
    }

    fn request(
        &self,
        url: &str,
        headers: &[(&str, &str)],
    ) -> Result<ureq::http::Response<ureq::Body>> {
        let mut request = self
            .agent
            .get(url)
            .config()
            .http_status_as_error(false)
            .build();
        for &(name, value) in headers {
            request = request.header(name, value);
        }
        request.call().with_context(|| format!("GET {url}"))
    }

    /// Stream a successful asset response without buffering the download.
    pub(crate) fn reader(
        &self,
        url: &str,
        headers: &[(&str, &str)],
    ) -> Result<Box<dyn std::io::Read>> {
        let response = self.request(url, headers)?;
        ensure!(
            response.status() == 200,
            "GET {url}: http status: {}",
            response.status().as_u16()
        );
        Ok(Box::new(response.into_body().into_reader()))
    }
}

impl HttpClient for UreqClient {
    fn get(&self, url: &str, headers: &[(&str, &str)]) -> Result<Response> {
        let response = self.request(url, headers)?;
        let status = response.status().as_u16();
        let response_headers = [
            "etag",
            "retry-after",
            "x-ratelimit-remaining",
            "x-ratelimit-reset",
        ]
        .into_iter()
        .filter_map(|name| {
            response
                .headers()
                .get(name)
                .and_then(|value| value.to_str().ok())
                .map(|value| (name.to_owned(), value.to_owned()))
        })
        .collect();
        let mut body = String::new();
        response
            .into_body()
            .into_reader()
            .take(u64::try_from(MAX_METADATA_BYTES)? + 1)
            .read_to_string(&mut body)
            .context("reading GitHub release metadata")?;
        ensure!(
            body.len() <= MAX_METADATA_BYTES,
            "GitHub response exceeds the {MAX_METADATA_BYTES}-byte limit"
        );
        Ok(Response {
            status,
            headers: response_headers,
            body,
        })
    }
}

#[derive(Debug, Deserialize, Serialize)]
struct Stamp {
    at: u64,
    boot: Option<String>,
}

impl Stamp {
    fn age(&self, now: u64, boot: Option<&str>) -> Option<u64> {
        if boot.is_some() && self.boot.as_deref() != boot {
            return None;
        }
        now.checked_sub(self.at)
    }
}

#[derive(Debug, Deserialize, Serialize)]
struct Entry {
    repository: String,
    stamp: Stamp,
    etag: Option<String>,
    body: String,
}

#[derive(Debug, Deserialize, Serialize)]
struct Cooldown {
    stamp: Stamp,
    until: u64,
}

/// Writes collected during discovery, published only outside preview.
#[derive(Debug, Default)]
pub(crate) struct CacheWrites(BTreeMap<PathBuf, String>);

impl CacheWrites {
    pub(crate) fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    /// Cache failures are advisory, but must remain visible.
    pub(crate) fn persist(&self) {
        for (path, content) in &self.0 {
            if let Err(error) = super::fs::write_atomic(path, content) {
                tracing::warn!(
                    "could not persist GitHub cache {}: {error:#}",
                    path.display()
                );
            }
        }
    }
}

/// One invocation's release lookups. Callers serialize network requests and
/// publish pending cache writes only when applying, never during discovery.
pub(crate) struct Releases<'a> {
    root: PathBuf,
    http: &'a dyn HttpClient,
    token: Option<String>,
    pending: CacheWrites,
    boot: Option<String>,
    clock: &'a dyn Fn() -> Result<u64>,
    cancelled: Option<super::cancellation::CancellationToken>,
}

impl std::fmt::Debug for Releases<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Releases")
            .field("root", &self.root)
            .field("authenticated", &self.token.is_some())
            .finish_non_exhaustive()
    }
}

impl<'a> Releases<'a> {
    pub(crate) fn new(root: &Path, http: &'a dyn HttpClient, token: Option<String>) -> Self {
        Self {
            root: root.join("bin/.github-releases-v1"),
            http,
            token,
            pending: CacheWrites::default(),
            boot: boot_id(),
            clock: &unix_timestamp,
            cancelled: None,
        }
    }

    pub(crate) fn with_cancellation(
        mut self,
        token: super::cancellation::CancellationToken,
    ) -> Self {
        self.cancelled = Some(token);
        self
    }

    fn check_cancelled(&self) -> Result<()> {
        if self
            .cancelled
            .as_ref()
            .is_some_and(super::cancellation::CancellationToken::is_cancelled)
        {
            return Err(super::exec::ExecError::Cancelled {
                command: "GitHub release discovery".into(),
                result: super::exec::ExecResult {
                    stdout: String::new(),
                    stderr: String::new(),
                    success: false,
                    code: None,
                },
            }
            .into());
        }
        Ok(())
    }

    fn path(&self, key: &str) -> PathBuf {
        let mut hash = Sha256::new();
        hash.update(key.as_bytes());
        hash.update([0]);
        if let Some(token) = &self.token {
            hash.update(token.as_bytes());
        }
        self.root.join(format!(
            "{}.json",
            base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(hash.finalize())
        ))
    }

    fn stamp(&self, now: u64) -> Stamp {
        Stamp {
            at: now,
            boot: self.boot.clone(),
        }
    }

    fn read<T: serde::de::DeserializeOwned>(&self, path: &Path) -> Option<T> {
        let result = if let Some(content) = self.pending.0.get(path) {
            serde_json::from_str(content).context("parsing pending GitHub cache")
        } else {
            let file = match std::fs::File::open(path) {
                Ok(file) => file,
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => return None,
                Err(error) => {
                    tracing::warn!("could not read GitHub cache {}: {error}", path.display());
                    return None;
                }
            };
            serde_json::from_reader(file.take(2 * 1024 * 1024)).context("parsing GitHub cache")
        };
        match result {
            Ok(value) => Some(value),
            Err(error) => {
                tracing::warn!(
                    "ignoring invalid GitHub cache {}: {error:#}",
                    path.display()
                );
                None
            }
        }
    }

    fn save(&mut self, path: PathBuf, value: &impl Serialize) -> Result<()> {
        self.pending.0.insert(path, serde_json::to_string(value)?);
        Ok(())
    }

    /// Fetch or revalidate metadata, caching only successful caller validation.
    /// A cooldown never turns a failed refresh into apparent success.
    pub(crate) fn latest<T>(
        &mut self,
        repository: &str,
        validate: impl Fn(&str) -> Result<T>,
    ) -> Result<T> {
        self.check_cancelled()?;
        ensure!(valid_repository(repository), "invalid GitHub repository");
        let path = self.path(repository);
        let requested_at = (self.clock)()?;
        let cached: Option<Entry> = self.read(&path);
        let cached = cached.filter(|entry| {
            entry.repository == repository
                && entry.body.len() <= MAX_METADATA_BYTES
                && validate(&entry.body).is_ok()
        });
        if let Some(entry) = &cached
            && entry
                .stamp
                .age(requested_at, self.boot.as_deref())
                .is_some_and(|age| age < CACHE_MAX_AGE)
        {
            return validate(&entry.body);
        }
        self.check_cooldown(requested_at)?;
        let url = format!("https://api.github.com/repos/{repository}/releases/latest");
        let authorization = self.token.as_ref().map(|token| format!("Bearer {token}"));
        let mut headers = vec![
            ("Accept", "application/vnd.github+json"),
            ("User-Agent", "dotfiles-cli"),
        ];
        if let Some(value) = &authorization {
            headers.push(("Authorization", value));
        }
        if let Some(etag) = cached.as_ref().and_then(|entry| entry.etag.as_deref()) {
            headers.push(("If-None-Match", etag));
        }
        let response = self.request(&url, &headers)?;
        let now = (self.clock)()?;
        let retry = cooldown_until(&response, now);
        if let Some(until) = retry {
            self.save(
                self.path("rate-limit"),
                &Cooldown {
                    stamp: self.stamp(now),
                    until,
                },
            )?;
        }
        if response.status != 200 && response.status != 304 {
            if let Some(until) = retry {
                bail!(
                    "GitHub API rate limited (HTTP {}); retry in {} seconds (Unix time {until})",
                    response.status,
                    until.saturating_sub(now)
                );
            }
            let authentication = if self.token.is_some() {
                "authenticated"
            } else {
                "anonymous"
            };
            bail!(
                "GitHub {authentication} release request failed (HTTP {}) for {repository}",
                response.status
            );
        }
        let (body, etag) = if response.status == 304 {
            let entry = cached.context("GitHub returned 304 without usable cached metadata")?;
            ensure!(
                entry.etag.is_some(),
                "GitHub returned 304 without a conditional request"
            );
            (
                entry.body,
                response.headers.get("etag").cloned().or(entry.etag),
            )
        } else {
            ensure!(
                response.body.len() <= MAX_METADATA_BYTES,
                "GitHub response exceeds the {MAX_METADATA_BYTES}-byte limit"
            );
            (response.body, response.headers.get("etag").cloned())
        };
        let result = validate(&body)?;
        self.save(
            path,
            &Entry {
                repository: repository.to_owned(),
                stamp: self.stamp(now),
                etag,
                body,
            },
        )?;
        Ok(result)
    }

    pub(crate) fn into_cache_writes(self) -> CacheWrites {
        self.pending
    }

    fn check_cooldown(&self, now: u64) -> Result<()> {
        if let Some(cooldown) = self.read::<Cooldown>(&self.path("rate-limit"))
            && cooldown.stamp.age(now, self.boot.as_deref()).is_some()
            && cooldown.until > now
        {
            bail!(
                "GitHub API rate-limit cooldown; retry in {} seconds (Unix time {})",
                cooldown.until - now,
                cooldown.until
            );
        }
        Ok(())
    }

    fn request(&self, url: &str, headers: &[(&str, &str)]) -> Result<Response> {
        for attempt in 1..=3 {
            self.check_cancelled()?;
            let result = self.http.get(url, headers);
            self.check_cancelled()?;
            let transient = match &result {
                Ok(response) => {
                    matches!(response.status, 408 | 500 | 502 | 503 | 504)
                        && cooldown_until(response, (self.clock)()?).is_none()
                }
                Err(error) => {
                    if error
                        .downcast_ref::<super::exec::ExecError>()
                        .is_some_and(super::exec::ExecError::is_cancelled)
                    {
                        return result;
                    }
                    let message = format!("{error:#}").to_ascii_lowercase();
                    [
                        "broken pipe",
                        "connection closed",
                        "connection refused",
                        "connection reset",
                        "dns",
                        "failed to lookup address",
                        "network is unreachable",
                        "timed out",
                        "temporary failure in name resolution",
                        "unexpected eof",
                    ]
                    .iter()
                    .any(|marker| message.contains(marker))
                }
            };
            if !transient || attempt == 3 {
                return result;
            }
            tracing::warn!(
                "transient GitHub release lookup failure; retrying (attempt {attempt}/3)"
            );
            #[cfg(not(test))]
            std::thread::sleep(Duration::from_secs(attempt));
        }
        bail!("GitHub release lookup attempts exhausted")
    }
}

fn cooldown_until(response: &Response, now: u64) -> Option<u64> {
    let exhausted = response
        .headers
        .get("x-ratelimit-remaining")
        .is_some_and(|value| value == "0");
    let limited = response.status == 429
        || (response.status == 403
            && (response.body.to_ascii_lowercase().contains("rate limit")
                || response
                    .body
                    .to_ascii_lowercase()
                    .contains("abuse detection")
                || response.headers.contains_key("retry-after")));
    if !exhausted && !limited {
        return None;
    }
    let retry = response
        .headers
        .get("retry-after")
        .and_then(|value| value.parse::<u64>().ok())
        .map(|seconds| now.saturating_add(seconds.max(1)));
    let reset = exhausted
        .then(|| {
            response
                .headers
                .get("x-ratelimit-reset")
                .and_then(|value| value.parse::<u64>().ok())
        })
        .flatten()
        .filter(|until| *until > now);
    Some(
        retry
            .into_iter()
            .chain(reset)
            .max()
            .unwrap_or_else(|| now.saturating_add(60)),
    )
}

fn valid_repository(repository: &str) -> bool {
    repository.split('/').count() == 2
        && repository.split('/').all(|part| {
            !part.is_empty()
                && part != "."
                && part != ".."
                && part
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b"-_.+".contains(&b))
        })
}

fn unix_timestamp() -> Result<u64> {
    Ok(SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .context("system clock precedes Unix epoch")?
        .as_secs())
}

#[cfg_attr(
    not(target_os = "linux"),
    allow(
        clippy::missing_const_for_fn,
        reason = "Linux implementation performs filesystem I/O"
    )
)]
fn boot_id() -> Option<String> {
    #[cfg(target_os = "linux")]
    {
        match std::fs::read_to_string("/proc/sys/kernel/random/boot_id") {
            Ok(value) => Some(value.trim().to_owned()),
            Err(error) => {
                tracing::debug!("boot identity unavailable for GitHub cache: {error}");
                None
            }
        }
    }
    #[cfg(not(target_os = "linux"))]
    {
        None
    }
}

#[cfg(test)]
pub(crate) mod tests;
