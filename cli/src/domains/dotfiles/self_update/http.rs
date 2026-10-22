//! HTTP plumbing for self-update: trait abstraction, real client, GitHub API
//! tag lookup, and SHA-256 checksum verification.

use std::io::{Read, Seek as _, Write as _};

use anyhow::{Context as _, Result, bail};
use sha2::{Digest, Sha256};

use super::REPO;

/// Number of attempts made for a single GET before the failure is reported.
const MAX_GET_ATTEMPTS: u32 = 3;
const MAX_RELEASE_METADATA_BYTES: usize = 1024 * 1024;
const MAX_CHECKSUM_BYTES: usize = 64 * 1024;

/// Failure fragments that indicate a retryable network or server condition.
///
/// Matching on the rendered error keeps the classification independent of the
/// concrete [`HttpClient`], so injected clients exercise the same code path as
/// [`UreqClient`].
const TRANSIENT_HTTP_ERROR_MARKERS: &[&str] = &[
    "broken pipe",
    "connection closed",
    "connection refused",
    "connection reset",
    "connection timed out",
    "dns",
    "failed to lookup address",
    "http status: 408",
    "http status: 425",
    "http status: 429",
    "http status: 500",
    "http status: 502",
    "http status: 503",
    "http status: 504",
    "network is unreachable",
    "operation timed out",
    "temporary failure in name resolution",
    "timed out",
    "unexpected eof",
];

/// Trait for making HTTP GET requests, enabling test injection.
///
/// Production code uses [`UreqClient`]; tests inject a mock that returns
/// predetermined responses without touching the network.
pub(super) trait HttpClient: std::fmt::Debug + Send + Sync {
    /// Perform an HTTP GET request and return an owned response-body reader.
    ///
    /// # Errors
    ///
    /// Returns an error on network failures, non-success status codes, or
    /// response-body read errors.
    fn get(&self, url: &str, headers: &[(&str, &str)]) -> Result<Box<dyn Read>>;
}

/// Real HTTP client backed by [`ureq`].
///
/// The agent is built once and reused for every request so that the three or
/// more GETs a single update performs (release tag, checksums file, asset)
/// share one connection pool instead of renegotiating TLS each time.
#[derive(Debug)]
pub(super) struct UreqClient {
    agent: ureq::Agent,
}

impl UreqClient {
    /// Create a new client whose requests share one agent and time out after
    /// `timeout_secs`.
    fn new(timeout_secs: u64) -> Self {
        let config = ureq::config::Config::builder()
            .timeout_global(Some(std::time::Duration::from_secs(timeout_secs)))
            .build();
        Self {
            agent: config.new_agent(),
        }
    }
}

impl HttpClient for UreqClient {
    fn get(&self, url: &str, headers: &[(&str, &str)]) -> Result<Box<dyn Read>> {
        let mut req = self.agent.get(url);
        for &(k, v) in headers {
            req = req.header(k, v);
        }
        let response = req.call().with_context(|| format!("GET {url}"))?;
        Ok(Box::new(response.into_body().into_reader()))
    }
}

/// Build the default HTTP client used by the self-update subsystem.
pub(super) fn default_http_client() -> UreqClient {
    UreqClient::new(120)
}

/// Perform a GET, retrying transient network and server failures.
///
/// Self-update runs unattended before mutating commands, so a single
/// dropped connection should not fail the run when a retry would succeed.
/// Permanent failures (404, checksum host misconfiguration, TLS errors) are
/// returned on the first attempt so genuine problems stay fast and visible.
fn get_with_retry<T>(
    client: &dyn HttpClient,
    url: &str,
    headers: &[(&str, &str)],
    mut read_body: impl FnMut(&mut dyn Read) -> Result<T>,
) -> Result<T> {
    let mut attempt = 1_u32;
    loop {
        match client.get(url, headers).and_then(|mut reader| {
            read_body(&mut *reader).with_context(|| format!("reading response from {url}"))
        }) {
            Ok(body) => return Ok(body),
            Err(error) if attempt < MAX_GET_ATTEMPTS && is_transient_http_error(&error) => {
                tracing::warn!(
                    "transient failure for GET {url} (attempt {attempt}/{MAX_GET_ATTEMPTS}), retrying: {error:#}"
                );
                std::thread::sleep(retry_delay(attempt));
                attempt = attempt.saturating_add(1);
            }
            Err(error) => return Err(error),
        }
    }
}

fn read_limited(reader: &mut dyn Read, limit: usize) -> Result<Vec<u8>> {
    let mut bytes = Vec::new();
    reader
        .take(u64::try_from(limit)?.saturating_add(1))
        .read_to_end(&mut bytes)?;
    anyhow::ensure!(
        bytes.len() <= limit,
        "HTTP response exceeds the {limit}-byte limit"
    );
    Ok(bytes)
}

/// Report whether `error` describes a condition worth retrying.
fn is_transient_http_error(error: &anyhow::Error) -> bool {
    let message = format!("{error:#}").to_ascii_lowercase();
    TRANSIENT_HTTP_ERROR_MARKERS
        .iter()
        .any(|marker| message.contains(marker))
}

#[cfg(not(test))]
fn retry_delay(attempt: u32) -> std::time::Duration {
    std::time::Duration::from_secs(u64::from(attempt))
}

#[cfg(test)]
const fn retry_delay(_attempt: u32) -> std::time::Duration {
    std::time::Duration::ZERO
}

/// Query the GitHub API for the latest release tag.
///
/// # Errors
///
/// Returns an error if the request fails after retries, or if the response is
/// not valid JSON. A successful response that carries no `tag_name` yields
/// `Ok(None)`.
pub(super) fn fetch_latest_tag(
    client: &dyn HttpClient,
    github_token: Option<&str>,
) -> Result<Option<String>> {
    let url = format!("https://api.github.com/repos/{REPO}/releases/latest");
    let authorization = github_token.map(|token| format!("Bearer {token}"));
    let mut headers = vec![
        ("Accept", "application/vnd.github.v3+json"),
        ("User-Agent", "dotfiles-cli"),
    ];
    if let Some(value) = authorization.as_deref() {
        headers.push(("Authorization", value));
    }

    let body_bytes = match get_with_retry(client, &url, &headers, |reader| {
        read_limited(reader, MAX_RELEASE_METADATA_BYTES)
    }) {
        Ok(body) => body,
        Err(error)
            if authorization.is_some()
                && (error_has_http_status(&error, 401) || error_has_http_status(&error, 403)) =>
        {
            tracing::debug!(
                "authenticated GitHub release check was denied, retrying anonymously: {error:#}"
            );
            get_with_retry(
                client,
                &url,
                &[
                    ("Accept", "application/vnd.github.v3+json"),
                    ("User-Agent", "dotfiles-cli"),
                ],
                |reader| read_limited(reader, MAX_RELEASE_METADATA_BYTES),
            )
            .with_context(|| format!("querying latest release anonymously from {url}"))?
        }
        Err(error) => {
            return Err(error).with_context(|| format!("querying latest release from {url}"));
        }
    };

    let parsed: serde_json::Value =
        serde_json::from_slice(&body_bytes).context("parsing GitHub API JSON response")?;
    Ok(parsed
        .get("tag_name")
        .and_then(serde_json::Value::as_str)
        .map(String::from))
}

fn error_has_http_status(error: &anyhow::Error, status: u16) -> bool {
    format!("{error:#}")
        .to_ascii_lowercase()
        .contains(&format!("http status: {status}"))
}

/// Stream a release asset into its staging file and return its SHA-256 digest.
///
/// A retry restarts both the file and the hash, including after a partial body.
pub(super) fn download_to_file(
    client: &dyn HttpClient,
    url: &str,
    file: &mut std::fs::File,
) -> Result<String> {
    get_with_retry(client, url, &[("User-Agent", "dotfiles-cli")], |reader| {
        file.set_len(0).context("clearing staged download")?;
        file.rewind().context("rewinding staged download")?;
        let mut hasher = Sha256::new();
        let mut buffer = vec![0_u8; 64 * 1024].into_boxed_slice();
        loop {
            let count = match reader.read(&mut buffer) {
                Ok(0) => break,
                Ok(count) => count,
                Err(error) if error.kind() == std::io::ErrorKind::Interrupted => continue,
                Err(error) => return Err(error.into()),
            };
            let bytes = buffer.split_at(count).0;
            file.write_all(bytes).context("writing staged download")?;
            hasher.update(bytes);
        }
        file.flush().context("flushing staged download")?;
        Ok(super::hex_encode(&hasher.finalize()))
    })
    .with_context(|| format!("downloading {url}"))
}

/// Verify the streamed SHA-256 digest against the checksums file for the
/// given release tag.
pub(super) fn verify_checksum(
    client: &dyn HttpClient,
    tag: &str,
    asset: &str,
    actual: &str,
) -> Result<()> {
    let url = format!("https://github.com/{REPO}/releases/download/{tag}/checksums.sha256");
    let checksums = get_with_retry(client, &url, &[("User-Agent", "dotfiles-cli")], |reader| {
        read_limited(reader, MAX_CHECKSUM_BYTES)
    })
    .context("downloading checksums file")?;
    let checksums_str = std::str::from_utf8(&checksums).context("decoding checksums file")?;

    let expected = checksums_str
        .lines()
        .find_map(|line| {
            let mut parts = line.split_whitespace();
            let hash = parts.next()?;
            let parsed_name = parts.collect::<Vec<_>>().join(" ");
            let stripped_name = parsed_name.strip_prefix('*').unwrap_or(&parsed_name);
            (stripped_name == asset).then(|| hash.to_string())
        })
        .ok_or_else(|| anyhow::anyhow!("checksum not found for {asset}"))?;

    if expected.len() != 64 || !expected.chars().all(|c| c.is_ascii_hexdigit()) {
        bail!("invalid checksum format for {asset}: expected 64 hex chars, got '{expected}'");
    }

    if !actual.eq_ignore_ascii_case(&expected) {
        bail!("checksum mismatch for {asset}: expected {expected}, got {actual}");
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Test-only mock and unit tests.
// ---------------------------------------------------------------------------

#[cfg(test)]
pub(super) mod test_support {
    use super::*;

    /// A mock HTTP client that returns pre-configured responses from a FIFO queue.
    #[derive(Debug)]
    pub(crate) struct MockHttpClient {
        responses: std::sync::Mutex<std::collections::VecDeque<Result<Vec<u8>>>>,
        requests: std::sync::Mutex<Vec<Vec<(String, String)>>>,
        urls: std::sync::Mutex<Vec<String>>,
    }

    impl MockHttpClient {
        /// Create a client that returns the given responses in order.
        pub(crate) fn new(responses: Vec<Result<Vec<u8>>>) -> Self {
            Self {
                responses: std::sync::Mutex::new(responses.into()),
                requests: std::sync::Mutex::new(Vec::new()),
                urls: std::sync::Mutex::new(Vec::new()),
            }
        }

        pub(crate) fn request_headers(&self) -> Vec<Vec<(String, String)>> {
            self.requests.lock().expect("mutex poisoned").clone()
        }

        pub(crate) fn request_urls(&self) -> Vec<String> {
            self.urls.lock().expect("mutex poisoned").clone()
        }
    }

    impl HttpClient for MockHttpClient {
        fn get(&self, url: &str, headers: &[(&str, &str)]) -> Result<Box<dyn Read>> {
            self.urls
                .lock()
                .expect("mutex poisoned")
                .push(url.to_string());
            self.requests.lock().expect("mutex poisoned").push(
                headers
                    .iter()
                    .map(|(name, value)| ((*name).to_string(), (*value).to_string()))
                    .collect(),
            );
            self.responses
                .lock()
                .expect("mutex poisoned")
                .pop_front()
                .unwrap_or_else(|| bail!("no more mock responses"))
                .map(|bytes| -> Box<dyn Read> { Box::new(std::io::Cursor::new(bytes)) })
        }
    }
}

#[cfg(test)]
mod tests {
    use super::test_support::MockHttpClient;
    use super::*;

    fn download_bytes(client: &dyn HttpClient, url: &str) -> Result<Vec<u8>> {
        let mut file = tempfile::NamedTempFile::new()?;
        let digest = download_to_file(client, url, file.as_file_mut())?;
        let bytes = std::fs::read(file.path())?;
        assert_eq!(digest, super::super::hex_encode(&Sha256::digest(&bytes)));
        Ok(bytes)
    }

    fn verify_checksum(
        client: &dyn HttpClient,
        tag: &str,
        asset: &str,
        bytes: &[u8],
    ) -> Result<()> {
        super::verify_checksum(
            client,
            tag,
            asset,
            &super::super::hex_encode(&Sha256::digest(bytes)),
        )
    }

    #[test]
    fn metadata_limits_accept_the_boundary_and_stop_at_one_extra_byte() {
        assert_eq!(MAX_RELEASE_METADATA_BYTES, 1_048_576);
        assert_eq!(MAX_CHECKSUM_BYTES, 65_536);
        for limit in [MAX_RELEASE_METADATA_BYTES, MAX_CHECKSUM_BYTES] {
            for length in [0, limit - 1, limit, limit + 1, limit * 2] {
                let mut reader = std::io::Cursor::new(vec![b'x'; length]);
                let result = read_limited(&mut reader, limit);
                assert_eq!(result.is_ok(), length <= limit, "{length}, limit={limit}");
                assert_eq!(
                    reader.position(),
                    u64::try_from(length.min(limit + 1)).unwrap()
                );
                if let Ok(bytes) = result {
                    assert_eq!(bytes.len(), length);
                }
            }
        }
    }

    #[test]
    fn oversized_metadata_fails_without_retrying_or_parsing_a_prefix() {
        let client = MockHttpClient::new(vec![Ok(vec![b' '; MAX_RELEASE_METADATA_BYTES + 1])]);
        let error = fetch_latest_tag(&client, None).unwrap_err();
        assert!(format!("{error:#}").contains("1048576-byte limit"));
        assert_eq!(client.request_urls().len(), 1);

        let checksum_client = MockHttpClient::new(vec![Ok(vec![b' '; MAX_CHECKSUM_BYTES + 1])]);
        let checksum_error =
            verify_checksum(&checksum_client, "v1", "asset", b"bytes").unwrap_err();
        assert!(format!("{checksum_error:#}").contains("65536-byte limit"));
        assert_eq!(checksum_client.request_urls().len(), 1);
    }

    #[test]
    fn a_partial_body_retry_restarts_the_file_and_hash() {
        use std::sync::atomic::{AtomicUsize, Ordering};
        struct BrokenBody {
            remaining: usize,
            failed: bool,
        }
        impl Read for BrokenBody {
            fn read(&mut self, buffer: &mut [u8]) -> std::io::Result<usize> {
                assert!(buffer.len() <= 65_536, "downloads must use bounded chunks");
                if self.remaining == 0 {
                    assert!(!self.failed, "a terminal body error must not be read again");
                    self.failed = true;
                    return Err(std::io::Error::new(
                        std::io::ErrorKind::ConnectionReset,
                        "connection reset",
                    ));
                }
                let count = self.remaining.min(buffer.len());
                buffer.split_at_mut(count).0.fill(b'x');
                self.remaining -= count;
                Ok(count)
            }
        }
        struct InterruptedBody {
            remaining: std::io::Cursor<&'static [u8]>,
            interrupted: bool,
        }
        impl Read for InterruptedBody {
            fn read(&mut self, buffer: &mut [u8]) -> std::io::Result<usize> {
                if self.remaining.position() > 0 && !self.interrupted {
                    self.interrupted = true;
                    return Err(std::io::ErrorKind::Interrupted.into());
                }
                let count = buffer.len().min(2);
                self.remaining.read(buffer.split_at_mut(count).0)
            }
        }
        #[derive(Debug)]
        struct PartialClient {
            attempts: AtomicUsize,
        }
        impl HttpClient for PartialClient {
            fn get(&self, _: &str, _: &[(&str, &str)]) -> Result<Box<dyn Read>> {
                if self.attempts.fetch_add(1, Ordering::Relaxed) == 0 {
                    Ok(Box::new(BrokenBody {
                        remaining: 128 * 1024,
                        failed: false,
                    }))
                } else {
                    Ok(Box::new(InterruptedBody {
                        remaining: std::io::Cursor::new(b"complete"),
                        interrupted: false,
                    }))
                }
            }
        }
        let client = PartialClient {
            attempts: AtomicUsize::new(0),
        };
        assert_eq!(
            download_bytes(&client, "https://example.invalid/asset").unwrap(),
            b"complete"
        );
        assert_eq!(client.attempts.load(Ordering::Relaxed), 2);
    }

    #[test]
    fn fetch_latest_tag_parses_github_response() {
        let client = MockHttpClient::new(vec![Ok(br#"{"tag_name": "v1.2.3"}"#.to_vec())]);
        let result = fetch_latest_tag(&client, None).unwrap();
        assert_eq!(result, Some("v1.2.3".to_string()));
        assert_eq!(
            client.request_urls(),
            [format!(
                "https://api.github.com/repos/{REPO}/releases/latest"
            )]
        );
    }

    #[test]
    fn fetch_latest_tag_uses_github_token_when_available() {
        let client = MockHttpClient::new(vec![Ok(br#"{"tag_name": "v1.2.3"}"#.to_vec())]);

        fetch_latest_tag(&client, Some("secret-token")).unwrap();

        let requests = client.request_headers();
        assert_eq!(requests.len(), 1);
        assert!(
            requests[0]
                .iter()
                .any(|(name, value)| name == "Authorization" && value == "Bearer secret-token"),
            "the release request should use the available GitHub token"
        );
    }

    #[test]
    fn fetch_latest_tag_falls_back_to_anonymous_when_token_is_rejected() {
        let client = MockHttpClient::new(vec![
            Err(anyhow::anyhow!("http status: 401")),
            Ok(br#"{"tag_name": "v1.2.3"}"#.to_vec()),
        ]);

        let result = fetch_latest_tag(&client, Some("expired-token")).unwrap();

        assert_eq!(result, Some("v1.2.3".to_string()));
        let requests = client.request_headers();
        assert_eq!(requests.len(), 2);
        assert!(
            requests[0].iter().any(|(name, _)| name == "Authorization"),
            "the first request should be authenticated"
        );
        assert!(
            requests[1].iter().all(|(name, _)| name != "Authorization"),
            "the retry should be anonymous"
        );
    }

    #[test]
    fn fetch_latest_tag_reports_network_error() {
        let client = MockHttpClient::new(vec![Err(anyhow::anyhow!("network error"))]);
        let error = fetch_latest_tag(&client, None).unwrap_err();
        assert!(
            format!("{error:#}").contains("network error"),
            "the underlying cause must survive so a broken update is diagnosable, got: {error:#}"
        );
    }

    #[test]
    fn fetch_latest_tag_returns_none_when_tag_name_missing() {
        let client = MockHttpClient::new(vec![Ok(br#"{"name": "Release v1.0"}"#.to_vec())]);
        let result = fetch_latest_tag(&client, None).unwrap();
        assert_eq!(result, None);
    }

    #[test]
    fn download_bytes_returns_response_body() {
        let client = MockHttpClient::new(vec![Ok(b"binary data".to_vec())]);
        let result = download_bytes(&client, "https://example.com/file").unwrap();
        assert_eq!(result, b"binary data");
        assert_eq!(client.request_urls(), ["https://example.com/file"]);
    }

    #[test]
    fn download_bytes_propagates_error() {
        let client = MockHttpClient::new(vec![Err(anyhow::anyhow!("timeout"))]);
        let result = download_bytes(&client, "https://example.com/file");
        assert!(result.is_err());
    }

    #[test]
    fn download_bytes_retries_transient_failure_then_succeeds() {
        let client = MockHttpClient::new(vec![
            Err(anyhow::anyhow!("connection reset by peer")),
            Ok(b"binary data".to_vec()),
        ]);
        let result = download_bytes(&client, "https://example.com/file").unwrap();
        assert_eq!(
            result, b"binary data",
            "a retried download should return the successful response body"
        );
        assert_eq!(client.request_urls(), ["https://example.com/file"; 2]);
    }

    #[test]
    fn download_bytes_gives_up_after_the_attempt_limit() {
        let responses = (0..MAX_GET_ATTEMPTS)
            .map(|_| Err(anyhow::anyhow!("http status: 503")))
            .collect();
        let client = MockHttpClient::new(responses);
        let error = download_bytes(&client, "https://example.com/file").unwrap_err();
        let msg = format!("{error:#}");
        assert!(
            msg.contains("503"),
            "the final transient failure should be reported, got: {msg}"
        );
        assert_eq!(
            client.request_urls(),
            vec!["https://example.com/file"; usize::try_from(MAX_GET_ATTEMPTS).unwrap()]
        );
    }

    #[test]
    fn download_bytes_does_not_retry_permanent_failures() {
        let client = MockHttpClient::new(vec![
            Err(anyhow::anyhow!("http status: 404")),
            Ok(b"never reached".to_vec()),
        ]);
        let error = download_bytes(&client, "https://example.com/file").unwrap_err();
        let msg = format!("{error:#}");
        assert!(
            msg.contains("404"),
            "a permanent failure should surface immediately, got: {msg}"
        );
        assert_eq!(client.request_urls(), ["https://example.com/file"]);
    }

    #[test]
    fn fetch_latest_tag_retries_transient_failure() {
        let client = MockHttpClient::new(vec![
            Err(anyhow::anyhow!("temporary failure in name resolution")),
            Ok(br#"{"tag_name": "v1.2.3"}"#.to_vec()),
        ]);
        let result = fetch_latest_tag(&client, None).unwrap();
        assert_eq!(
            result,
            Some("v1.2.3".to_string()),
            "a transient DNS failure should not be treated as offline"
        );
    }

    #[test]
    fn verify_checksum_succeeds_with_matching_hash() {
        let data = b"hello world";
        let mut hasher = Sha256::new();
        hasher.update(data);
        let hash = super::super::hex_encode(&hasher.finalize());

        let checksums = format!("{hash}  test-asset\n");
        let client = MockHttpClient::new(vec![Ok(checksums.into_bytes())]);

        verify_checksum(&client, "v1.0.0", "test-asset", data).unwrap();
        assert_eq!(
            client.request_urls(),
            [format!(
                "https://github.com/{REPO}/releases/download/v1.0.0/checksums.sha256"
            )]
        );
    }

    #[test]
    fn verify_checksum_fails_with_wrong_hash() {
        let checksums =
            "deadbeef0123456789abcdef0123456789abcdef0123456789abcdef01234567  test-asset\n";
        let client = MockHttpClient::new(vec![Ok(checksums.as_bytes().to_vec())]);

        let result = verify_checksum(&client, "v1.0.0", "test-asset", b"hello");
        assert!(result.is_err());
        let msg = format!("{:#}", result.unwrap_err());
        assert!(
            msg.contains("checksum mismatch"),
            "expected 'checksum mismatch' in: {msg}"
        );
    }

    #[test]
    fn verify_checksum_fails_when_asset_not_in_checksums() {
        let checksums = "abc123  other-asset\n";
        let client = MockHttpClient::new(vec![Ok(checksums.as_bytes().to_vec())]);

        let result = verify_checksum(&client, "v1.0.0", "missing-asset", b"data");
        assert!(result.is_err());
        let msg = format!("{:#}", result.unwrap_err());
        assert!(
            msg.contains("checksum not found"),
            "expected 'checksum not found' in: {msg}"
        );
    }

    #[test]
    fn verify_checksum_succeeds_when_asset_name_contains_spaces() {
        let data = b"hello world";
        let mut hasher = Sha256::new();
        hasher.update(data);
        let hash = super::super::hex_encode(&hasher.finalize());

        let checksums = format!("{hash}  release build/test asset\n");
        let client = MockHttpClient::new(vec![Ok(checksums.into_bytes())]);

        verify_checksum(&client, "v1.0.0", "release build/test asset", data).unwrap();
    }

    #[test]
    fn verify_checksum_succeeds_with_uppercase_hash() {
        let data = b"hello world";
        let mut hasher = Sha256::new();
        hasher.update(data);
        let hash = super::super::hex_encode_upper(&hasher.finalize());

        let checksums = format!("{hash}  test-asset\n");
        let client = MockHttpClient::new(vec![Ok(checksums.into_bytes())]);

        verify_checksum(&client, "v1.0.0", "test-asset", data).unwrap();
    }

    #[test]
    fn verify_checksum_fails_with_malformed_hash() {
        let checksums = "tooshort  test-asset\n";
        let client = MockHttpClient::new(vec![Ok(checksums.as_bytes().to_vec())]);

        let result = verify_checksum(&client, "v1.0.0", "test-asset", b"data");
        assert!(result.is_err());
        let msg = format!("{:#}", result.unwrap_err());
        assert!(
            msg.contains("invalid checksum format"),
            "expected 'invalid checksum format' in: {msg}"
        );
    }
}
