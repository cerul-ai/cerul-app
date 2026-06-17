use std::{io, net::IpAddr, time::Duration};

use url::Url;

pub(crate) fn validate_external_http_url(value: &str, label: &str) -> anyhow::Result<Url> {
    let parsed =
        Url::parse(value.trim()).map_err(|_| anyhow::anyhow!("{label} is not a valid URL"))?;
    anyhow::ensure!(
        matches!(parsed.scheme(), "http" | "https"),
        "{label} must be http or https"
    );
    anyhow::ensure!(
        parsed.username().is_empty() && parsed.password().is_none(),
        "{label} must not include credentials"
    );
    let host = parsed
        .host_str()
        .ok_or_else(|| anyhow::anyhow!("{label} is missing a host"))?;
    anyhow::ensure!(
        !host_is_internal(host),
        "{label} must not target local or private network hosts"
    );
    Ok(parsed)
}

pub(crate) fn safe_http_client(label: &'static str) -> anyhow::Result<reqwest::Client> {
    Ok(reqwest::Client::builder()
        .timeout(Duration::from_secs(60))
        .connect_timeout(Duration::from_secs(10))
        .redirect(reqwest::redirect::Policy::custom(move |attempt| {
            if attempt.previous().len() >= 10 {
                return attempt.error(io::Error::new(
                    io::ErrorKind::PermissionDenied,
                    format!("{label} exceeded redirect limit"),
                ));
            }
            if let Err(error) = validate_external_http_url(attempt.url().as_str(), label) {
                return attempt.error(io::Error::new(io::ErrorKind::PermissionDenied, error));
            }
            attempt.follow()
        }))
        .build()?)
}

fn host_is_internal(host: &str) -> bool {
    let normalized = host.trim_end_matches('.').to_ascii_lowercase();
    if normalized == "localhost"
        || normalized.ends_with(".localhost")
        || normalized.ends_with(".local")
        || normalized == "metadata.google.internal"
    {
        return true;
    }

    normalized.parse::<IpAddr>().is_ok_and(ip_addr_is_internal)
}

fn ip_addr_is_internal(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(ip) => {
            let octets = ip.octets();
            ip.is_loopback()
                || ip.is_private()
                || ip.is_link_local()
                || ip.is_unspecified()
                || ip.is_broadcast()
                || octets[0] == 100 && (octets[1] & 0b1100_0000) == 0b0100_0000
        }
        IpAddr::V6(ip) => {
            ip.is_loopback()
                || ip.is_unspecified()
                || ip.is_unique_local()
                || ip.is_unicast_link_local()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_internal_or_credentialed_http_urls() {
        for url in [
            "file:///tmp/feed.xml",
            "http://localhost/feed.xml",
            "http://127.0.0.1/feed.xml",
            "http://10.0.0.5/feed.xml",
            "https://169.254.169.254/latest",
            "https://user:pass@example.com/feed.xml",
        ] {
            assert!(
                validate_external_http_url(url, "test URL").is_err(),
                "expected {url} to be rejected"
            );
        }
    }

    #[test]
    fn accepts_public_http_urls() {
        assert!(validate_external_http_url("https://example.com/feed.xml", "test URL").is_ok());
        assert!(validate_external_http_url("http://example.com/feed.xml", "test URL").is_ok());
    }
}
