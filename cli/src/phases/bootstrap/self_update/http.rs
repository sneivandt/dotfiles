//! HTTP plumbing for self-update: trait abstraction, real client, GitHub API
//! tag lookup, and SHA-256 checksum verification.

use std::io::Read;

use anyhow::{Context as _, Result, bail};
use sha2::{Digest, Sha256};

use super::REPO;

/// Trait for making HTTP GET requests, enabling test injection.
///
/// Production code uses [`UreqClient`]; tests inject a mock that returns
/// predetermined responses without touching the network.
pub(super) trait HttpClient: std::fmt::Debug + Send + Sync {
    /// Perform an HTTP GET request and return the response body as bytes.
    ///
    /// # Errors
    ///
    /// Returns an error on network failures, non-success status codes, or
    /// response-body read errors.
    fn get(&self, url: &str, headers: &[(&str, &str)]) -> Result<Vec<u8>>;
}

/// Real HTTP client backed by [`ureq`].
#[derive(Debug)]
pub(super) struct UreqClient {
    /// Global request timeout in seconds.
    timeout_secs: u64,
}

impl UreqClient {
    /// Create a new client with the given timeout.
    const fn new(timeout_secs: u64) -> Self {
        Self { timeout_secs }
    }
}

impl HttpClient for UreqClient {
    fn get(&self, url: &str, headers: &[(&str, &str)]) -> Result<Vec<u8>> {
        let config = ureq::config::Config::builder()
            .timeout_global(Some(std::time::Duration::from_secs(self.timeout_secs)))
            .build();
        let agent = config.new_agent();
        let mut req = agent.get(url);
        for &(k, v) in headers {
            req = req.header(k, v);
        }
        let response = req.call().with_context(|| format!("GET {url}"))?;
        let mut buf = Vec::new();
        response
            .into_body()
            .into_reader()
            .read_to_end(&mut buf)
            .with_context(|| format!("reading response from {url}"))?;
        Ok(buf)
    }
}

/// Build the default HTTP client used by the self-update subsystem.
pub(super) const fn default_http_client() -> UreqClient {
    UreqClient::new(120)
}

/// Query the GitHub API for the latest release tag.
pub(super) fn fetch_latest_tag(client: &dyn HttpClient) -> Result<Option<String>> {
    let url = format!("https://api.github.com/repos/{REPO}/releases/latest");
    let Ok(body_bytes) = client.get(
        &url,
        &[
            ("Accept", "application/vnd.github.v3+json"),
            ("User-Agent", "dotfiles-cli"),
        ],
    ) else {
        return Ok(None);
    };

    let body = String::from_utf8_lossy(&body_bytes);
    let parsed: serde_json::Value =
        serde_json::from_str(&body).context("parsing GitHub API JSON response")?;
    Ok(parsed
        .get("tag_name")
        .and_then(serde_json::Value::as_str)
        .map(String::from))
}

/// Download a URL and return the bytes.
pub(super) fn download_bytes(client: &dyn HttpClient, url: &str) -> Result<Vec<u8>> {
    client
        .get(url, &[("User-Agent", "dotfiles-cli")])
        .with_context(|| format!("downloading {url}"))
}

/// Verify the SHA-256 checksum of `data` against the checksums file for the
/// given release tag.
pub(super) fn verify_checksum(
    client: &dyn HttpClient,
    tag: &str,
    asset: &str,
    data: &[u8],
) -> Result<()> {
    let url = format!("https://github.com/{REPO}/releases/download/{tag}/checksums.sha256");
    let checksums = download_bytes(client, &url).context("downloading checksums file")?;
    let checksums_str = String::from_utf8_lossy(&checksums);

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

    let mut hasher = Sha256::new();
    hasher.update(data);
    let actual = format!("{:x}", hasher.finalize());

    if !actual.eq_ignore_ascii_case(&expected) {
        bail!("checksum mismatch for {asset}: expected {expected}, got {actual}");
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Test-only mock and unit tests.
// ---------------------------------------------------------------------------

#[cfg(test)]
#[allow(clippy::expect_used, clippy::unwrap_used, clippy::indexing_slicing)]
pub(super) mod test_support {
    use super::*;

    /// A mock HTTP client that returns pre-configured responses from a FIFO queue.
    #[derive(Debug)]
    pub struct MockHttpClient {
        responses: std::sync::Mutex<std::collections::VecDeque<Result<Vec<u8>>>>,
    }

    impl MockHttpClient {
        /// Create a client that returns the given responses in order.
        pub fn new(responses: Vec<Result<Vec<u8>>>) -> Self {
            Self {
                responses: std::sync::Mutex::new(responses.into()),
            }
        }
    }

    impl HttpClient for MockHttpClient {
        fn get(&self, _url: &str, _headers: &[(&str, &str)]) -> Result<Vec<u8>> {
            self.responses
                .lock()
                .expect("mutex poisoned")
                .pop_front()
                .unwrap_or_else(|| bail!("no more mock responses"))
        }
    }
}

#[cfg(test)]
#[allow(clippy::expect_used, clippy::unwrap_used, clippy::indexing_slicing)]
mod tests {
    use super::test_support::MockHttpClient;
    use super::*;

    #[test]
    fn fetch_latest_tag_parses_github_response() {
        let client = MockHttpClient::new(vec![Ok(br#"{"tag_name": "v1.2.3"}"#.to_vec())]);
        let result = fetch_latest_tag(&client).unwrap();
        assert_eq!(result, Some("v1.2.3".to_string()));
    }

    #[test]
    fn fetch_latest_tag_returns_none_on_network_error() {
        let client = MockHttpClient::new(vec![Err(anyhow::anyhow!("network error"))]);
        let result = fetch_latest_tag(&client).unwrap();
        assert_eq!(result, None);
    }

    #[test]
    fn fetch_latest_tag_returns_none_when_tag_name_missing() {
        let client = MockHttpClient::new(vec![Ok(br#"{"name": "Release v1.0"}"#.to_vec())]);
        let result = fetch_latest_tag(&client).unwrap();
        assert_eq!(result, None);
    }

    #[test]
    fn download_bytes_returns_response_body() {
        let client = MockHttpClient::new(vec![Ok(b"binary data".to_vec())]);
        let result = download_bytes(&client, "https://example.com/file").unwrap();
        assert_eq!(result, b"binary data");
    }

    #[test]
    fn download_bytes_propagates_error() {
        let client = MockHttpClient::new(vec![Err(anyhow::anyhow!("timeout"))]);
        let result = download_bytes(&client, "https://example.com/file");
        assert!(result.is_err());
    }

    #[test]
    fn verify_checksum_succeeds_with_matching_hash() {
        let data = b"hello world";
        let mut hasher = Sha256::new();
        hasher.update(data);
        let hash = format!("{:x}", hasher.finalize());

        let checksums = format!("{hash}  test-asset\n");
        let client = MockHttpClient::new(vec![Ok(checksums.into_bytes())]);

        verify_checksum(&client, "v1.0.0", "test-asset", data).unwrap();
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
        let hash = format!("{:x}", hasher.finalize());

        let checksums = format!("{hash}  release build/test asset\n");
        let client = MockHttpClient::new(vec![Ok(checksums.into_bytes())]);

        verify_checksum(&client, "v1.0.0", "release build/test asset", data).unwrap();
    }

    #[test]
    fn verify_checksum_succeeds_with_uppercase_hash() {
        let data = b"hello world";
        let mut hasher = Sha256::new();
        hasher.update(data);
        let hash = format!("{:X}", hasher.finalize());

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
