//! How the client reaches a registry. [`Http`] is the real one; tests
//! serve answers from memory through the same trait.

use std::io::Read as _;
use std::time::Duration;

/// An answer to a [`Transport::get`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Reply {
    /// The resource still matches the entity tag sent.
    NotModified,
    /// The resource's bytes, and its entity tag when it has one.
    Body {
        /// The bytes.
        bytes: Vec<u8>,
        /// The `etag` header.
        etag: Option<String>,
    },
}

/// Fetches registry resources.
pub trait Transport: Send + Sync {
    /// GETs `url`, reading at most `max_bytes`. With `etag`, the request
    /// is conditional and may answer [`Reply::NotModified`].
    ///
    /// # Errors
    ///
    /// The URL is refused, the server cannot be reached or answers
    /// other than 2xx or 304, or the body is longer than `max_bytes`.
    fn get(&self, url: &str, etag: Option<&str>, max_bytes: u64) -> Result<Reply, String>;
}

/// HTTPS through kage's SSRF-guarded agent, so a registry can never
/// point the client at an internal address, not even by redirect.
#[derive(Debug)]
pub struct Http {
    agent: ureq::Agent,
}

impl Default for Http {
    fn default() -> Self {
        let config = ureq::Agent::config_builder()
            .timeout_global(Some(Duration::from_secs(60)))
            .http_status_as_error(false)
            .build();
        Self {
            agent: kage_tools::ssrf::guarded_agent(config),
        }
    }
}

impl Transport for Http {
    fn get(&self, url: &str, etag: Option<&str>, max_bytes: u64) -> Result<Reply, String> {
        if !url.starts_with("https://") {
            return Err(format!("{url}: a registry is reached over https only"));
        }
        let mut request = self
            .agent
            .get(url)
            .header("user-agent", concat!("kage/", env!("CARGO_PKG_VERSION")));
        if let Some(etag) = etag {
            request = request.header("if-none-match", etag);
        }
        let response = request.call().map_err(|e| format!("get {url}: {e}"))?;
        let status = response.status().as_u16();
        if status == 304 {
            return Ok(Reply::NotModified);
        }
        if !(200..300).contains(&status) {
            return Err(format!("get {url}: the server answered {status}"));
        }
        let etag = response
            .headers()
            .get("etag")
            .and_then(|value| value.to_str().ok())
            .map(str::to_owned);
        let mut bytes = Vec::new();
        response
            .into_body()
            .into_reader()
            .take(max_bytes.saturating_add(1))
            .read_to_end(&mut bytes)
            .map_err(|e| format!("read {url}: {e}"))?;
        if bytes.len() as u64 > max_bytes {
            return Err(format!("{url} is larger than {max_bytes} bytes"));
        }
        Ok(Reply::Body { bytes, etag })
    }
}
