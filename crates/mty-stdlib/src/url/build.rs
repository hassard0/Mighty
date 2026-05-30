//! Fluent URL builder. Constructs a [`Url`](super::Url) piece-by-piece.

use super::encode::percent_encode_component;
use super::parse::{Url, UrlErr};

/// Builder-style URL construction.
///
/// ```
/// # use mty_stdlib::url::Url;
/// let u = Url::builder("https")
///     .host("example.com")
///     .port(8443)
///     .path("/api/v1/items")
///     .query_param("q", "hello world")
///     .query_param("page", "2")
///     .build()
///     .unwrap();
/// assert!(u.to_string().starts_with("https://example.com:8443/api/v1/items?"));
/// ```
#[derive(Debug, Clone)]
pub struct UrlBuilder {
    scheme: String,
    username: String,
    password: String,
    host: String,
    port: Option<u16>,
    path: String,
    query: Vec<(String, String)>,
    fragment: String,
}

impl UrlBuilder {
    #[must_use]
    pub fn new(scheme: &str) -> Self {
        Self {
            scheme: scheme.to_string(),
            username: String::new(),
            password: String::new(),
            host: String::new(),
            port: None,
            path: String::new(),
            query: Vec::new(),
            fragment: String::new(),
        }
    }

    #[must_use]
    pub fn userinfo(mut self, user: &str, password: &str) -> Self {
        self.username = user.to_string();
        self.password = password.to_string();
        self
    }

    #[must_use]
    pub fn host(mut self, host: &str) -> Self {
        self.host = host.to_string();
        self
    }

    #[must_use]
    pub fn port(mut self, port: u16) -> Self {
        self.port = Some(port);
        self
    }

    #[must_use]
    pub fn path(mut self, path: &str) -> Self {
        self.path = if path.starts_with('/') || path.is_empty() {
            path.to_string()
        } else {
            format!("/{}", path)
        };
        self
    }

    #[must_use]
    pub fn query_param(mut self, key: &str, value: &str) -> Self {
        self.query.push((key.to_string(), value.to_string()));
        self
    }

    #[must_use]
    pub fn fragment(mut self, frag: &str) -> Self {
        self.fragment = frag.to_string();
        self
    }

    /// Finalise the builder.
    pub fn build(self) -> Result<Url, UrlErr> {
        if self.scheme.is_empty() {
            return Err(UrlErr::Build("scheme is required".into()));
        }
        // Encode the query pairs into the canonical "k=v&k2=v2" form.
        let query = if self.query.is_empty() {
            String::new()
        } else {
            self.query
                .iter()
                .map(|(k, v)| {
                    format!(
                        "{}={}",
                        percent_encode_component(k),
                        percent_encode_component(v)
                    )
                })
                .collect::<Vec<_>>()
                .join("&")
        };
        Ok(Url {
            scheme: self.scheme,
            username: self.username,
            password: self.password,
            host: self.host,
            port: self.port,
            path: if self.path.is_empty() {
                "/".to_string()
            } else {
                self.path
            },
            query,
            fragment: self.fragment,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn minimal_builder() {
        let u = UrlBuilder::new("https")
            .host("example.com")
            .build()
            .unwrap();
        assert_eq!(u.scheme, "https");
        assert_eq!(u.host, "example.com");
        assert_eq!(u.path, "/");
    }

    #[test]
    fn full_builder() {
        let u = UrlBuilder::new("https")
            .userinfo("alice", "secret")
            .host("vault.example.com")
            .port(8443)
            .path("/api/items")
            .query_param("q", "hello world")
            .query_param("page", "2")
            .fragment("top")
            .build()
            .unwrap();
        assert_eq!(u.scheme, "https");
        assert_eq!(u.username, "alice");
        assert_eq!(u.password, "secret");
        assert_eq!(u.host, "vault.example.com");
        assert_eq!(u.port, Some(8443));
        assert_eq!(u.path, "/api/items");
        assert_eq!(u.query, "q=hello%20world&page=2");
        assert_eq!(u.fragment, "top");
    }

    #[test]
    fn builder_path_normalises_leading_slash() {
        let u = UrlBuilder::new("https")
            .host("example.com")
            .path("api/v1")
            .build()
            .unwrap();
        assert_eq!(u.path, "/api/v1");
    }

    #[test]
    fn rejects_empty_scheme() {
        let err = UrlBuilder::new("").host("example.com").build().unwrap_err();
        assert!(format!("{}", err).contains("scheme"));
    }

    #[test]
    fn round_trip_via_to_string() {
        let u = UrlBuilder::new("https")
            .host("example.com")
            .port(9000)
            .path("/api")
            .query_param("k", "v")
            .build()
            .unwrap();
        assert_eq!(u.to_string(), "https://example.com:9000/api?k=v");
    }

    #[test]
    fn query_keys_with_special_chars_are_encoded() {
        let u = UrlBuilder::new("https")
            .host("example.com")
            .query_param("a&b", "c=d")
            .build()
            .unwrap();
        // Both key and value are component-encoded so the resulting
        // query is unambiguous.
        assert_eq!(u.query, "a%26b=c%3Dd");
    }

    #[test]
    fn empty_path_defaults_to_slash() {
        let u = UrlBuilder::new("https")
            .host("example.com")
            .build()
            .unwrap();
        assert_eq!(u.path, "/");
    }
}
