//! Website address validation for `open_website`. Pure and fully tested.
//!
//! Only ordinary public https sites are allowed: no other schemes, no IP addresses or
//! local hostnames (keeps the agent off routers and local services), no embedded login
//! details, no unusual ports. The result is always an absolute `https://` URL, so it can
//! never be mistaken for a browser command-line flag.

use url::{Host, Url};

const MAX_URL_CHARS: usize = 2048;

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct SafeUrl {
    /// Normalised absolute URL, always `https://…`.
    pub url: String,
    /// Lowercase host, e.g. `www.bbc.co.uk`.
    pub host: String,
}

impl SafeUrl {
    /// Host without a leading `www.`, for comparing with what the browser shows.
    pub fn site(&self) -> &str {
        self.host.strip_prefix("www.").unwrap_or(&self.host)
    }
}

pub fn validate_url(input: &str) -> Result<SafeUrl, String> {
    let raw = input.trim();
    if raw.is_empty() {
        return Err("No website address was given.".into());
    }
    if raw.chars().count() > MAX_URL_CHARS {
        return Err("That website address is too long.".into());
    }
    if raw
        .chars()
        .any(|c| c.is_whitespace() || c.is_control() || c == '"' || c == '\\')
    {
        return Err("That website address contains spaces or unusual characters.".into());
    }
    let lower = raw.to_ascii_lowercase();
    let with_scheme = if lower.starts_with("https://") {
        raw.to_string()
    } else if lower.starts_with("http://") {
        // Upgrade: the browser will say if a site really has no secure version.
        format!("https://{}", &raw["http://".len()..])
    } else if raw.contains("://")
        || lower.starts_with("javascript:")
        || lower.starts_with("data:")
        || lower.starts_with("file:")
    {
        return Err("Only normal web addresses (https) can be opened.".into());
    } else {
        format!("https://{raw}")
    };

    let url = Url::parse(&with_scheme)
        .map_err(|_| "That doesn't look like a website address.".to_string())?;
    if url.scheme() != "https" {
        return Err("Only normal web addresses (https) can be opened.".into());
    }
    if !url.username().is_empty() || url.password().is_some() {
        return Err("Website addresses with login details in them aren't allowed.".into());
    }
    if url.port().is_some() {
        return Err("Website addresses with unusual port numbers aren't allowed.".into());
    }
    let host = match url.host() {
        Some(Host::Domain(d)) => d.to_ascii_lowercase(),
        Some(Host::Ipv4(_)) | Some(Host::Ipv6(_)) => {
            return Err(
                "Opening sites by number (IP address) isn't allowed. Please use the site's name."
                    .into(),
            );
        }
        None => return Err("That doesn't look like a website address.".into()),
    };
    let labels: Vec<&str> = host.trim_end_matches('.').split('.').collect();
    if labels.len() < 2 || labels.iter().any(|l| l.is_empty()) {
        return Err("That doesn't look like a full website name (for example bbc.co.uk).".into());
    }
    let tld = labels[labels.len() - 1];
    if matches!(
        tld,
        "local" | "localhost" | "internal" | "lan" | "home" | "corp" | "intranet"
    ) {
        return Err("Local network addresses aren't allowed.".into());
    }
    Ok(SafeUrl {
        url: url.to_string(),
        host: host.trim_end_matches('.').to_string(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accepts_ordinary_sites_and_normalises_to_https() {
        let u = validate_url("bbc.co.uk").unwrap();
        assert_eq!(u.url, "https://bbc.co.uk/");
        assert_eq!(u.host, "bbc.co.uk");
        assert_eq!(
            validate_url("https://www.example.com/a?b=c")
                .unwrap()
                .site(),
            "example.com"
        );
        assert_eq!(
            validate_url("http://example.com").unwrap().url,
            "https://example.com/"
        );
        assert_eq!(validate_url(" Example.COM ").unwrap().host, "example.com");
    }

    #[test]
    fn international_names_become_punycode() {
        assert_eq!(
            validate_url("https://bücher.de").unwrap().host,
            "xn--bcher-kva.de"
        );
    }

    #[test]
    fn refuses_dangerous_or_odd_addresses() {
        for bad in [
            "",
            "javascript:alert(1)",
            "data:text/html,<b>x</b>",
            "file:///C:/Windows/System32",
            "ftp://example.com",
            "chrome://settings",
            "edge://settings",
            "https://192.168.1.1",
            "https://[::1]/",
            "https://localhost",
            "https://printer.local",
            "https://user:pass@example.com",
            "https://example.com:8443",
            "https://example.com/ hello",
            "--disable-web-security",
            "intranet",
            "https://.com",
        ] {
            assert!(validate_url(bad).is_err(), "{bad:?} should be refused");
        }
    }

    #[test]
    fn result_can_never_look_like_a_command_line_flag() {
        for ok in [
            "example.com",
            "http://example.com/--flag",
            "https://example.com/?x=--y",
        ] {
            assert!(validate_url(ok).unwrap().url.starts_with("https://"));
        }
    }
}
