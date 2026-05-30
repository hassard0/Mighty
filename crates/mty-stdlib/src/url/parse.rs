//! URL parsing — `Url::parse(s)` returns a struct with named fields.

use ::url::Url as ExtUrl;

/// Parsed URL components. Field shapes match what reads well in Mighty
/// source — `host` is a string (empty if absent), `port` is an
/// `Option<u16>`, and `query` / `fragment` are bare strings (empty when
/// absent) to avoid forcing every consumer to unwrap an `Option`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Url {
    pub scheme: String,
    pub username: String,
    pub password: String,
    pub host: String,
    pub port: Option<u16>,
    pub path: String,
    pub query: String,
    pub fragment: String,
}

impl Url {
    /// Parse `s` into a [`Url`]. Returns [`UrlErr`] on RFC 3986 / WHATWG
    /// rejection.
    pub fn parse(s: &str) -> Result<Self, UrlErr> {
        parse(s)
    }

    /// Builder for fluent URL construction.
    #[must_use]
    pub fn builder(scheme: &str) -> super::build::UrlBuilder {
        super::build::UrlBuilder::new(scheme)
    }
}

impl std::fmt::Display for Url {
    /// Render this URL back to a canonical string form.
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.scheme)?;
        f.write_str(":")?;
        if !self.host.is_empty() || self.scheme != "data" {
            f.write_str("//")?;
            if !self.username.is_empty() || !self.password.is_empty() {
                f.write_str(&self.username)?;
                if !self.password.is_empty() {
                    f.write_str(":")?;
                    f.write_str(&self.password)?;
                }
                f.write_str("@")?;
            }
            f.write_str(&self.host)?;
            if let Some(p) = self.port {
                write!(f, ":{}", p)?;
            }
        }
        f.write_str(&self.path)?;
        if !self.query.is_empty() {
            f.write_str("?")?;
            f.write_str(&self.query)?;
        }
        if !self.fragment.is_empty() {
            f.write_str("#")?;
            f.write_str(&self.fragment)?;
        }
        Ok(())
    }
}

/// Free-function form mirroring `Url::parse`. Exported for the Mighty
/// surface (`std.url.parse(s)`).
pub fn parse(s: &str) -> Result<Url, UrlErr> {
    let u = ExtUrl::parse(s).map_err(|e| UrlErr::Parse(e.to_string()))?;
    Ok(Url {
        scheme: u.scheme().to_string(),
        username: u.username().to_string(),
        password: u.password().unwrap_or("").to_string(),
        host: u.host_str().unwrap_or("").to_string(),
        port: u.port(),
        path: u.path().to_string(),
        query: u.query().unwrap_or("").to_string(),
        fragment: u.fragment().unwrap_or("").to_string(),
    })
}

#[derive(Debug, thiserror::Error)]
pub enum UrlErr {
    #[error("url parse: {0}")]
    Parse(String),
    #[error("url build: {0}")]
    Build(String),
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn full_https_url() {
        let u = parse("https://user:pass@example.com:8443/path/sub?q=hello&n=1#frag").unwrap();
        assert_eq!(u.scheme, "https");
        assert_eq!(u.username, "user");
        assert_eq!(u.password, "pass");
        assert_eq!(u.host, "example.com");
        assert_eq!(u.port, Some(8443));
        assert_eq!(u.path, "/path/sub");
        assert_eq!(u.query, "q=hello&n=1");
        assert_eq!(u.fragment, "frag");
    }

    #[test]
    fn minimal_http_url() {
        let u = parse("http://example.com").unwrap();
        assert_eq!(u.scheme, "http");
        assert_eq!(u.host, "example.com");
        assert_eq!(u.port, None);
        // WHATWG normalises the path to "/" when absent.
        assert_eq!(u.path, "/");
        assert_eq!(u.query, "");
        assert_eq!(u.fragment, "");
    }

    #[test]
    fn query_with_special_chars_percent_encoded() {
        // The WHATWG URL parser percent-encodes spaces in the query.
        let u = parse("https://example.com/?q=hello world").unwrap();
        assert_eq!(u.query, "q=hello%20world");
    }

    #[test]
    fn ipv4_host() {
        let u = parse("http://127.0.0.1:8080/").unwrap();
        assert_eq!(u.host, "127.0.0.1");
        assert_eq!(u.port, Some(8080));
    }

    #[test]
    fn ipv6_host_bracketed() {
        let u = parse("http://[::1]:8080/").unwrap();
        // WHATWG normalises the brackets in the host_str view.
        assert!(u.host.contains("::1"));
        assert_eq!(u.port, Some(8080));
    }

    #[test]
    fn default_port_omitted() {
        // url crate hides the default port when it matches the scheme.
        let u = parse("https://example.com:443/").unwrap();
        assert_eq!(u.port, None);
        let u = parse("http://example.com:80/").unwrap();
        assert_eq!(u.port, None);
    }

    #[test]
    fn non_default_port_present() {
        let u = parse("https://example.com:9000/").unwrap();
        assert_eq!(u.port, Some(9000));
    }

    #[test]
    fn file_url_no_host() {
        let u = parse("file:///tmp/foo.txt").unwrap();
        assert_eq!(u.scheme, "file");
        assert_eq!(u.path, "/tmp/foo.txt");
    }

    #[test]
    fn mailto_url() {
        let u = parse("mailto:user@example.com").unwrap();
        assert_eq!(u.scheme, "mailto");
        // Opaque path stays as-is.
        assert_eq!(u.path, "user@example.com");
    }

    #[test]
    fn fragment_only() {
        let u = parse("https://example.com/#section").unwrap();
        assert_eq!(u.fragment, "section");
    }

    #[test]
    fn percent_encoded_path() {
        let u = parse("https://example.com/he%20llo").unwrap();
        assert_eq!(u.path, "/he%20llo");
    }

    #[test]
    fn case_insensitive_scheme() {
        let u = parse("HTTPS://example.com/").unwrap();
        assert_eq!(u.scheme, "https");
    }

    #[test]
    fn rejects_garbage() {
        assert!(parse("not a url").is_err());
        assert!(parse("://nope").is_err());
        assert!(parse("").is_err());
    }

    #[test]
    fn url_to_string_round_trips_simple() {
        let s = "https://example.com/path";
        let u = parse(s).unwrap();
        assert_eq!(u.to_string(), s);
    }

    #[test]
    fn url_to_string_round_trips_with_query_fragment() {
        let u = Url {
            scheme: "https".into(),
            username: String::new(),
            password: String::new(),
            host: "example.com".into(),
            port: Some(9000),
            path: "/api".into(),
            query: "k=v".into(),
            fragment: "anchor".into(),
        };
        assert_eq!(u.to_string(), "https://example.com:9000/api?k=v#anchor");
    }

    #[test]
    fn url_to_string_userinfo() {
        let u = Url {
            scheme: "https".into(),
            username: "alice".into(),
            password: "secret".into(),
            host: "vault.example.com".into(),
            port: None,
            path: "/".into(),
            query: String::new(),
            fragment: String::new(),
        };
        assert_eq!(u.to_string(), "https://alice:secret@vault.example.com/");
    }

    #[test]
    fn url_method_form_matches_free_function() {
        let a = Url::parse("https://example.com/").unwrap();
        let b = parse("https://example.com/").unwrap();
        assert_eq!(a, b);
    }

    #[test]
    fn user_info_without_password() {
        let u = parse("https://alice@example.com/").unwrap();
        assert_eq!(u.username, "alice");
        assert_eq!(u.password, "");
    }

    #[test]
    fn ws_scheme() {
        let u = parse("wss://example.com/ws").unwrap();
        assert_eq!(u.scheme, "wss");
        assert_eq!(u.path, "/ws");
    }

    #[test]
    fn long_path_preserved() {
        let s = "https://example.com/a/very/deep/path/with/many/segments/file.txt";
        let u = parse(s).unwrap();
        assert_eq!(u.path, "/a/very/deep/path/with/many/segments/file.txt");
    }

    #[test]
    fn error_message_is_descriptive() {
        let err = parse("not a url").unwrap_err();
        let msg = format!("{}", err);
        assert!(msg.starts_with("url parse:"), "msg: {}", msg);
    }

    #[test]
    fn double_slash_scheme_only() {
        let u = parse("ftp://files.example.com/share/file").unwrap();
        assert_eq!(u.scheme, "ftp");
        assert_eq!(u.host, "files.example.com");
        assert_eq!(u.path, "/share/file");
    }

    #[test]
    fn empty_query_separator_preserved_as_empty() {
        // "https://example.com/?" parses with empty query string but the
        // presence of the '?' marker is preserved by the underlying lib.
        let u = parse("https://example.com/?").unwrap();
        assert_eq!(u.query, "");
    }

    #[test]
    fn query_with_multiple_pairs() {
        let u = parse("https://example.com/search?q=mty&page=2&sort=desc").unwrap();
        assert_eq!(u.query, "q=mty&page=2&sort=desc");
    }
}
