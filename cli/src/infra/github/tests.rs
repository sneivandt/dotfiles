use super::*;
use std::collections::VecDeque;
use std::sync::Mutex;

type Request = (String, Vec<(String, String)>);

#[derive(Debug)]
pub(crate) struct MockHttpClient {
    responses: Mutex<VecDeque<Result<Response>>>,
    requests: Mutex<Vec<Request>>,
}

impl MockHttpClient {
    pub(crate) fn new(responses: Vec<Result<Vec<u8>>>) -> Self {
        Self::responses(
            responses
                .into_iter()
                .map(|response| {
                    response.map(|bytes| Response {
                        status: 200,
                        body: String::from_utf8(bytes).unwrap(),
                        ..Response::default()
                    })
                })
                .collect(),
        )
    }

    pub(crate) fn responses(responses: Vec<Result<Response>>) -> Self {
        Self {
            responses: Mutex::new(responses.into()),
            requests: Mutex::new(Vec::new()),
        }
    }

    pub(crate) fn request_urls(&self) -> Vec<String> {
        self.requests
            .lock()
            .unwrap()
            .iter()
            .map(|(url, _)| url.clone())
            .collect()
    }

    pub(crate) fn request_headers(&self) -> Vec<Vec<(String, String)>> {
        self.requests
            .lock()
            .unwrap()
            .iter()
            .map(|(_, headers)| headers.clone())
            .collect()
    }
}

impl HttpClient for MockHttpClient {
    fn get(&self, url: &str, headers: &[(&str, &str)]) -> Result<Response> {
        self.requests.lock().unwrap().push((
            url.to_owned(),
            headers
                .iter()
                .map(|(key, value)| ((*key).to_owned(), (*value).to_owned()))
                .collect(),
        ));
        self.responses
            .lock()
            .unwrap()
            .pop_front()
            .expect("unexpected HTTP request")
    }
}

pub(crate) fn response(status: u16, headers: &[(&str, &str)], body: &str) -> Response {
    Response {
        status,
        headers: headers
            .iter()
            .map(|(key, value)| ((*key).to_owned(), (*value).to_owned()))
            .collect(),
        body: body.to_owned(),
    }
}

fn tag(body: &str) -> Result<String> {
    let value: serde_json::Value = serde_json::from_str(body)?;
    value
        .get("tag_name")
        .and_then(serde_json::Value::as_str)
        .map(str::to_owned)
        .context("missing tag")
}

const BODY: &str = r#"{"tag_name":"v1"}"#;

#[test]
fn debug_identifies_release_lookup_without_exposing_credentials_or_responses() {
    let dir = tempfile::tempdir().unwrap();
    let http = MockHttpClient::new(vec![Ok(b"private-response-sentinel".to_vec())]);
    for token in [None, Some("credential-sentinel".to_owned())] {
        let authenticated = token.is_some();
        let releases = Releases::new(dir.path(), &http, token);
        let debug = format!("{releases:?}");
        assert!(debug.starts_with("Releases {"), "{debug}");
        assert!(
            debug.contains(&format!("root: {:?}", releases.root)),
            "{debug}"
        );
        assert!(
            debug.contains(&format!("authenticated: {authenticated}")),
            "{debug}"
        );
        assert!(!debug.contains("credential-sentinel"), "{debug}");
        assert!(!debug.contains("private-response-sentinel"), "{debug}");
    }
}

#[test]
fn default_cache_clock_and_boot_identity_match_native_sources() {
    let dir = tempfile::tempdir().unwrap();
    let http = MockHttpClient::new(vec![]);
    let releases = Releases::new(dir.path(), &http, None);
    let before = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_secs();
    let timestamp = (releases.clock)().unwrap();
    let after = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_secs();
    assert!((before..=after).contains(&timestamp), "{timestamp}");
    let expected_boot = if cfg!(target_os = "linux") {
        std::fs::read_to_string("/proc/sys/kernel/random/boot_id")
            .ok()
            .map(|value| value.trim().to_owned())
    } else {
        None
    };
    assert_eq!(releases.boot, expected_boot);
    assert_eq!(releases.stamp(timestamp).boot, expected_boot);
}

#[test]
fn missing_cache_is_quiet_but_unreadable_cache_is_reported() {
    let dir = tempfile::tempdir().unwrap();
    let http = MockHttpClient::new(vec![]);
    let releases = Releases::new(dir.path(), &http, None);
    let (log, _logs, _guard) = crate::infra::logging::isolated_logger();
    let log_path = log.log_path().unwrap();
    assert!(
        releases
            .read::<Entry>(&releases.path("example/tool"))
            .is_none()
    );
    let missing_log = std::fs::read_to_string(log_path).unwrap();
    assert!(!missing_log.contains("[warn]"), "{missing_log}");

    let invalid_path = dir.path().join("invalid-cache\0.json");
    assert!(releases.read::<Entry>(&invalid_path).is_none());
    let error_log = std::fs::read_to_string(log_path).unwrap();
    assert!(
        error_log.contains("could not read GitHub cache"),
        "{error_log}"
    );
    assert!(error_log.contains("invalid-cache"), "{error_log}");
}

#[test]
fn large_valid_metadata_survives_cache_serialization_overhead() {
    let dir = tempfile::tempdir().unwrap();
    let body = serde_json::json!({
        "tag_name": "v1",
        "padding": "x".repeat(MAX_METADATA_BYTES - 100),
    })
    .to_string();
    assert!(body.len() <= MAX_METADATA_BYTES);
    let http = MockHttpClient::new(vec![Ok(body.into_bytes())]);
    let mut releases = Releases::new(dir.path(), &http, None);
    releases.clock = &|| Ok(100);
    assert!(releases.pending.is_empty());
    assert_eq!(releases.latest("example/tool", tag).unwrap(), "v1");
    assert!(!releases.pending.is_empty());
    let path = releases.path("example/tool");
    releases.into_cache_writes().persist();
    assert!(
        std::fs::metadata(path).unwrap().len() > u64::try_from(MAX_METADATA_BYTES + 2).unwrap()
    );

    let mut cached = Releases::new(dir.path(), &http, None);
    cached.clock = &|| Ok(101);
    assert_eq!(cached.latest("example/tool", tag).unwrap(), "v1");
    assert!(cached.into_cache_writes().is_empty());
    assert_eq!(http.request_urls().len(), 1);
}

#[test]
fn metadata_is_authenticated_cached_and_conditionally_revalidated() {
    let dir = tempfile::tempdir().unwrap();
    let http = MockHttpClient::responses(vec![
        Ok(response(200, &[("etag", "\"first\"")], BODY)),
        Ok(response(304, &[], "")),
    ]);
    let clock = std::cell::Cell::new(100);
    let now = || Ok(clock.get());
    let mut releases = Releases::new(dir.path(), &http, Some("fixture-token".into()));
    releases.clock = &now;
    assert_eq!(releases.latest("example/tool", tag).unwrap(), "v1");
    assert!(
        !releases.root.exists(),
        "discovery must not write cache files"
    );
    assert_eq!(releases.latest("example/tool", tag).unwrap(), "v1");
    assert_eq!(
        http.request_urls().len(),
        1,
        "same-invocation lookups share cache"
    );
    releases.pending.persist();
    let entry_path = releases.path("example/tool");
    assert!(
        !std::fs::read_to_string(&entry_path)
            .unwrap()
            .contains("fixture-token")
    );
    clock.set(699);
    let mut fresh = Releases::new(dir.path(), &http, Some("fixture-token".into()));
    fresh.clock = &now;
    assert_eq!(fresh.latest("example/tool", tag).unwrap(), "v1");
    assert_eq!(
        http.request_urls().len(),
        1,
        "fresh persisted metadata skips HTTP"
    );
    clock.set(700);
    assert_eq!(fresh.latest("example/tool", tag).unwrap(), "v1");
    let headers = http.request_headers();
    assert_eq!(headers.len(), 2);
    assert!(headers[0].contains(&("Authorization".into(), "Bearer fixture-token".into())));
    assert!(!headers[0].iter().any(|(key, _)| key == "If-None-Match"));
    assert!(headers[1].contains(&("If-None-Match".into(), "\"first\"".into())));
    fresh.pending.persist();
    let stored: Entry =
        serde_json::from_str(&std::fs::read_to_string(entry_path).unwrap()).unwrap();
    assert_eq!(stored.stamp.at, 700);
    assert_eq!(stored.body, BODY);
}

#[test]
fn cooldown_is_shared_across_repositories_and_persisted_but_auth_scoped() {
    let dir = tempfile::tempdir().unwrap();
    let http = MockHttpClient::responses(vec![
        Ok(response(
            403,
            &[("x-ratelimit-remaining", "0"), ("x-ratelimit-reset", "200")],
            "",
        )),
        Ok(response(200, &[], BODY)),
        Ok(response(200, &[], BODY)),
    ]);
    let now = || Ok(100);
    let mut releases = Releases::new(dir.path(), &http, None);
    releases.clock = &now;
    let error = releases.latest("example/a", tag).unwrap_err();
    assert!(error.to_string().contains("rate limited (HTTP 403)"));
    assert!(error.to_string().contains("100 seconds"));
    assert!(
        releases
            .latest("example/b", tag)
            .unwrap_err()
            .to_string()
            .contains("cooldown")
    );
    assert_eq!(http.request_urls().len(), 1);
    releases.pending.persist();
    let mut next = Releases::new(dir.path(), &http, None);
    next.clock = &now;
    assert!(
        next.latest("example/c", tag)
            .unwrap_err()
            .to_string()
            .contains("cooldown")
    );
    assert_eq!(http.request_urls().len(), 1);
    let mut authenticated = Releases::new(dir.path(), &http, Some("token".into()));
    authenticated.clock = &now;
    assert_eq!(authenticated.latest("example/c", tag).unwrap(), "v1");
    let expired = || Ok(200);
    next.clock = &expired;
    assert_eq!(next.latest("example/c", tag).unwrap(), "v1");
    assert_eq!(http.request_urls().len(), 3);
}

#[test]
fn cooldown_headers_and_non_rate_limit_denials_are_distinguished() {
    for (status, headers, body, expected) in [
        (429, vec![], "", Some(160)),
        (
            403,
            vec![],
            r#"{"message":"API rate limit exceeded"}"#,
            Some(160),
        ),
        (403, vec![("retry-after", "120")], "", Some(220)),
        (403, vec![("retry-after", "bad")], "", Some(160)),
        (403, vec![("retry-after", "0")], "", Some(101)),
        (
            403,
            vec![("x-ratelimit-remaining", "0"), ("x-ratelimit-reset", "90")],
            "",
            Some(160),
        ),
        (
            403,
            vec![("x-ratelimit-remaining", "0"), ("x-ratelimit-reset", "100")],
            "",
            Some(160),
        ),
        (
            403,
            vec![
                ("x-ratelimit-remaining", "0"),
                ("x-ratelimit-reset", "300"),
                ("retry-after", "120"),
            ],
            "",
            Some(300),
        ),
        (
            200,
            vec![("x-ratelimit-remaining", "0"), ("x-ratelimit-reset", "200")],
            BODY,
            Some(200),
        ),
        (403, vec![], "Forbidden", None),
        (401, vec![], "Bad credentials", None),
    ] {
        assert_eq!(
            cooldown_until(&response(status, &headers, body), 100),
            expected,
            "{status} {headers:?} {body}"
        );
    }
    for status in [401, 403, 404] {
        let dir = tempfile::tempdir().unwrap();
        let http = MockHttpClient::responses(vec![Ok(response(status, &[], "denied"))]);
        let mut releases = Releases::new(dir.path(), &http, Some("token".into()));
        let error = releases.latest("example/tool", tag).unwrap_err();
        assert!(
            error
                .to_string()
                .contains("authenticated release request failed")
        );
        assert!(!error.to_string().contains("rate limit"));
        assert_eq!(
            http.request_urls().len(),
            1,
            "denials must not retry anonymously"
        );
        assert!(
            releases.pending.is_empty(),
            "ordinary denials must not poison the quota cache"
        );
    }
}

#[test]
fn invalid_cache_inputs_or_metadata_do_not_hide_network_or_validation_failures() {
    for case in ["future", "boot", "repository", "corrupt", "descriptor"] {
        let dir = tempfile::tempdir().unwrap();
        let http = MockHttpClient::responses(vec![Ok(response(200, &[], r#"{"tag_name":"v2"}"#))]);
        let mut releases = Releases::new(dir.path(), &http, None);
        releases.clock = &|| Ok(100);
        releases.boot = Some("boot-b".into());
        let path = releases.path("example/tool");
        let entry = Entry {
            repository: if case == "repository" {
                "wrong/repo"
            } else {
                "example/tool"
            }
            .into(),
            stamp: Stamp {
                at: if case == "future" { 200 } else { 99 },
                boot: Some(if case == "boot" { "boot-a" } else { "boot-b" }.into()),
            },
            etag: None,
            body: BODY.into(),
        };
        let content = if case == "corrupt" {
            "broken".into()
        } else {
            serde_json::to_string(&entry).unwrap()
        };
        super::super::fs::write_atomic(&path, content).unwrap();
        let result = releases
            .latest("example/tool", |body| {
                let value = tag(body)?;
                ensure!(case != "descriptor" || value == "v2", "changed descriptor");
                Ok(value)
            })
            .unwrap();
        assert_eq!(result, "v2", "{case}");
        assert_eq!(http.request_urls().len(), 1, "{case}");
    }
    for body in ["broken", "{}", &" ".repeat(MAX_METADATA_BYTES + 1)] {
        let dir = tempfile::tempdir().unwrap();
        let http = MockHttpClient::responses(vec![Ok(response(200, &[], body))]);
        let mut releases = Releases::new(dir.path(), &http, None);
        assert!(releases.latest("example/tool", tag).is_err());
        assert!(
            releases.pending.is_empty(),
            "invalid metadata cannot become fresh cache"
        );
    }
}

#[test]
fn stale_metadata_is_not_a_success_fallback_and_unsolicited_304_fails() {
    for status in [403, 304] {
        let dir = tempfile::tempdir().unwrap();
        let http = MockHttpClient::responses(vec![Ok(response(status, &[], ""))]);
        let mut releases = Releases::new(dir.path(), &http, None);
        releases.clock = &|| Ok(1000);
        let entry = Entry {
            repository: "example/tool".into(),
            stamp: releases.stamp(100),
            etag: None,
            body: BODY.into(),
        };
        let path = releases.path("example/tool");
        super::super::fs::write_atomic(&path, serde_json::to_string(&entry).unwrap()).unwrap();
        assert!(releases.latest("example/tool", tag).is_err(), "{status}");
        assert!(releases.pending.is_empty());
        assert_eq!(http.request_urls().len(), 1);
    }
}

#[test]
fn fresh_metadata_remains_usable_during_cooldown() {
    let dir = tempfile::tempdir().unwrap();
    let http = MockHttpClient::responses(vec![Ok(response(
        200,
        &[("x-ratelimit-remaining", "0"), ("x-ratelimit-reset", "200")],
        BODY,
    ))]);
    let mut releases = Releases::new(dir.path(), &http, None);
    releases.clock = &|| Ok(100);
    assert_eq!(releases.latest("example/tool", tag).unwrap(), "v1");
    assert_eq!(releases.latest("example/tool", tag).unwrap(), "v1");
    assert!(releases.latest("example/other", tag).is_err());
    assert_eq!(http.request_urls().len(), 1);
}

#[test]
fn malformed_repository_is_rejected_before_io() {
    let dir = tempfile::tempdir().unwrap();
    let http = MockHttpClient::new(vec![]);
    let mut releases = Releases::new(dir.path(), &http, None);
    for repo in [
        "",
        "owner",
        "../repo",
        "owner/..",
        "owner/repo/extra",
        "owner/repo?x",
    ] {
        assert!(releases.latest(repo, tag).is_err(), "{repo}");
    }
    assert!(http.request_urls().is_empty());
    assert!(!releases.root.exists());
}

#[test]
fn transient_failures_retry_with_a_bound_but_cancellation_does_not() {
    let dir = tempfile::tempdir().unwrap();
    let http = MockHttpClient::responses(vec![
        Err(anyhow::anyhow!("temporary failure in name resolution")),
        Ok(response(503, &[], "")),
        Ok(response(200, &[], BODY)),
    ]);
    assert_eq!(
        Releases::new(dir.path(), &http, None)
            .latest("example/tool", tag)
            .unwrap(),
        "v1"
    );
    assert_eq!(http.request_urls().len(), 3);
    let exhausted = MockHttpClient::responses((0..3).map(|_| Ok(response(503, &[], ""))).collect());
    assert!(
        Releases::new(dir.path(), &exhausted, None)
            .latest("example/tool", tag)
            .unwrap_err()
            .to_string()
            .contains("503")
    );
    assert_eq!(exhausted.request_urls().len(), 3);
    let cancelled = super::super::cancellation::CancellationToken::new();
    cancelled.cancel();
    let no_requests = MockHttpClient::new(vec![]);
    let error = Releases::new(dir.path(), &no_requests, None)
        .with_cancellation(cancelled)
        .latest("example/tool", tag)
        .unwrap_err();
    assert!(
        error
            .downcast_ref::<super::super::exec::ExecError>()
            .unwrap()
            .is_cancelled()
    );
    assert!(no_requests.request_urls().is_empty());
}

fn serve_once(wire: String) -> (String, std::thread::JoinHandle<String>) {
    use std::io::{BufRead as _, Write as _};
    let listener = std::net::TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, 0)).unwrap();
    let url = format!("http://{}/release", listener.local_addr().unwrap());
    let server = std::thread::spawn(move || {
        let (mut stream, _) = listener.accept().unwrap();
        stream
            .set_read_timeout(Some(Duration::from_secs(5)))
            .unwrap();
        let mut reader = std::io::BufReader::new(&stream);
        let mut request = String::new();
        loop {
            let mut line = String::new();
            assert_ne!(
                reader.read_line(&mut line).unwrap(),
                0,
                "HTTP request must contain headers"
            );
            if line == "\r\n" {
                break;
            }
            request.push_str(&line);
            assert!(request.len() < 8192, "fixture request must be bounded");
        }
        stream.write_all(wire.as_bytes()).unwrap();
        request
    });
    (url, server)
}

#[test]
fn native_transport_preserves_status_and_headers_and_bounds_metadata() {
    let http = UreqClient::new(Duration::from_secs(5));
    let (url, server) = serve_once(format!(
        "HTTP/1.1 403 Forbidden\r\nRetry-After: 120\r\nX-RateLimit-Remaining: 0\r\nX-RateLimit-Reset: 999\r\nETag: \"tag\"\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{BODY}",
        BODY.len()
    ));
    let reply = http
        .get(&url, &[("Authorization", "Bearer fixture-token")])
        .unwrap();
    assert_eq!(reply.status, 403);
    assert_eq!(reply.headers["retry-after"], "120");
    assert_eq!(reply.headers["x-ratelimit-remaining"], "0");
    assert_eq!(reply.headers["x-ratelimit-reset"], "999");
    assert_eq!(reply.headers["etag"], "\"tag\"");
    assert_eq!(reply.body, BODY);
    assert!(
        server
            .join()
            .unwrap()
            .to_ascii_lowercase()
            .contains("authorization: bearer fixture-token")
    );
    for size in [MAX_METADATA_BYTES, MAX_METADATA_BYTES + 1] {
        let body = "x".repeat(size);
        let (bounded_url, bounded_server) = serve_once(format!(
            "HTTP/1.1 200 OK\r\nContent-Length: {size}\r\nConnection: close\r\n\r\n{body}"
        ));
        let result = http.get(&bounded_url, &[]);
        if size == MAX_METADATA_BYTES {
            assert_eq!(result.unwrap().body.len(), size);
        } else {
            assert!(
                result
                    .unwrap_err()
                    .to_string()
                    .contains("1048576-byte limit")
            );
        }
        bounded_server.join().unwrap();
    }
}
