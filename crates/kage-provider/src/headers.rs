//! Shared request-header assembly for the built-in providers.

use std::collections::BTreeMap;

/// Headers every provider request carries: the protocol's `fixed`
/// headers, the credential header when a key is configured (`None`
/// for local endpoints with an empty key), then the configured
/// extras in key order.
pub(crate) fn request_headers(
    fixed: &[(&'static str, String)],
    credential: Option<(&'static str, String)>,
    extras: &BTreeMap<String, String>,
) -> Vec<(String, String)> {
    let mut headers: Vec<_> = fixed
        .iter()
        .map(|(name, value)| ((*name).to_owned(), value.clone()))
        .collect();
    if let Some((name, value)) = credential {
        headers.push((name.to_owned(), value));
    }
    headers.extend(
        extras
            .iter()
            .map(|(name, value)| (name.clone(), value.clone())),
    );
    headers
}

/// Headers `OpenCode` needs on requests to its own endpoints: the
/// conversation it routes the request by and the calling client. A
/// request outside a session gets a fresh id, as `OpenCode` does for
/// its one-off generations. Headers the user configured win.
pub(crate) fn opencode_headers(
    base_url: &str,
    session: Option<&str>,
    extras: &BTreeMap<String, String>,
) -> Vec<(String, String)> {
    if !is_opencode(base_url) {
        return Vec::new();
    }
    let session = session.map_or_else(|| kage_core::SessionId::new().to_string(), str::to_owned);
    [
        ("x-opencode-session", session),
        ("x-opencode-client", "kage".to_owned()),
    ]
    .into_iter()
    .filter(|(name, _)| !extras.keys().any(|key| key.eq_ignore_ascii_case(name)))
    .map(|(name, value)| (name.to_owned(), value))
    .collect()
}

fn is_opencode(base_url: &str) -> bool {
    let rest = base_url
        .split_once("://")
        .map_or(base_url, |(_, rest)| rest);
    let host = rest.split(['/', ':']).next().unwrap_or_default();
    host.eq_ignore_ascii_case("opencode.ai") || host.to_ascii_lowercase().ends_with(".opencode.ai")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn extras() -> BTreeMap<String, String> {
        BTreeMap::from([
            ("x-b".to_owned(), "2".to_owned()),
            ("x-a".to_owned(), "1".to_owned()),
        ])
    }

    #[test]
    fn opencode_endpoints_get_the_session_and_client() {
        let headers = opencode_headers(
            "https://opencode.ai/zen/go/v1",
            Some("01SESSION"),
            &BTreeMap::new(),
        );
        assert_eq!(
            headers,
            [
                ("x-opencode-session", "01SESSION"),
                ("x-opencode-client", "kage"),
            ]
            .map(|(name, value)| (name.to_owned(), value.to_owned()))
        );
        assert_eq!(
            opencode_headers("https://api.opencode.ai", None, &BTreeMap::new()).len(),
            2
        );
    }

    #[test]
    fn other_hosts_get_no_opencode_headers() {
        for url in [
            "https://api.openai.com/v1",
            "https://opencode.ai.example.com/v1",
            "https://notopencode.ai/v1",
        ] {
            assert!(
                opencode_headers(url, Some("s"), &BTreeMap::new()).is_empty(),
                "{url}"
            );
        }
    }

    #[test]
    fn a_configured_opencode_header_wins() {
        let extras = BTreeMap::from([("X-OpenCode-Client".to_owned(), "mine".to_owned())]);
        let headers = opencode_headers("https://opencode.ai/zen/v1", Some("s"), &extras);
        assert_eq!(headers, [("x-opencode-session".to_owned(), "s".to_owned())]);
    }

    #[test]
    fn fixed_then_credential_then_extras_in_key_order() {
        let headers = request_headers(
            &[
                ("content-type", "application/json".to_owned()),
                ("anthropic-version", "2023-06-01".to_owned()),
            ],
            Some(("x-api-key", "k".to_owned())),
            &extras(),
        );
        assert_eq!(
            headers,
            [
                ("content-type", "application/json"),
                ("anthropic-version", "2023-06-01"),
                ("x-api-key", "k"),
                ("x-a", "1"),
                ("x-b", "2"),
            ]
            .map(|(name, value)| (name.to_owned(), value.to_owned()))
        );
    }

    #[test]
    fn an_empty_key_omits_the_credential_header() {
        let headers = request_headers(
            &[("content-type", "application/json".to_owned())],
            None,
            &extras(),
        );
        assert_eq!(
            headers,
            [
                ("content-type", "application/json"),
                ("x-a", "1"),
                ("x-b", "2"),
            ]
            .map(|(name, value)| (name.to_owned(), value.to_owned()))
        );
    }
}
