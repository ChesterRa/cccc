//! A push endpoint is a URL the browser supplies and the Web process will
//! POST to, so only the push services browsers use are accepted: no other
//! host, no address literal, no credentials and no other port.

use url::{Host, Url};

const MAX_LEN: usize = 2048;
/// Chrome and other Chromium browsers, Firefox, and Safari.
const HOSTS: &[&str] = &[
    "fcm.googleapis.com",
    "updates.push.services.mozilla.com",
    "web.push.apple.com",
];
/// Edge's Windows Notification Service hosts and Apple's other push hosts.
const HOST_SUFFIXES: &[&str] = &[".notify.windows.com", ".push.apple.com"];
/// Chrome Beta, Dev and Canary subscribe through Google's staging endpoint
/// (Chromium's `kPushMessagingStagingGcmEndpoint`); only that path is taken.
const STAGING_HOST: &str = "jmt17.google.com";
const STAGING_PATH: &str = "/fcm/send/";

pub(crate) fn validate(endpoint: &str) -> Option<Url> {
    if endpoint.len() > MAX_LEN {
        return None;
    }
    let url = Url::parse(endpoint).ok()?;
    if url.scheme() != "https"
        || !url.username().is_empty()
        || url.password().is_some()
        || url.port().is_some()
        || url.fragment().is_some()
    {
        return None;
    }
    let Some(Host::Domain(host)) = url.host() else {
        return None;
    };
    let host = host.to_ascii_lowercase();
    (HOSTS.contains(&host.as_str())
        || HOST_SUFFIXES.iter().any(|suffix| host.ends_with(suffix))
        || (host == STAGING_HOST && url.path().starts_with(STAGING_PATH)))
    .then_some(url)
}

/// The push service origin a VAPID token is addressed to.
pub(crate) fn audience(url: &Url) -> String {
    url.origin().ascii_serialization()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_browser_push_services_are_accepted() {
        for endpoint in [
            "https://fcm.googleapis.com/fcm/send/abc:def",
            "https://updates.push.services.mozilla.com/wpush/v2/gAAAA",
            "https://web.push.apple.com/QGuQyavXut",
            "https://wns2-par02p.notify.windows.com/w/?token=BQYAAAB",
            "https://FCM.googleapis.com/fcm/send/abc",
            "https://jmt17.google.com/fcm/send/abc:def",
        ] {
            assert!(validate(endpoint).is_some(), "{endpoint}");
        }
        for endpoint in [
            "http://fcm.googleapis.com/fcm/send/abc",
            "https://fcm.googleapis.com:8443/fcm/send/abc",
            "https://user:secret@fcm.googleapis.com/fcm/send/abc",
            "https://fcm.googleapis.com.example.com/fcm/send/abc",
            "https://notify.windows.com.example.com/w/",
            "https://evil-notify.windows.com.attacker.net/w/",
            "https://127.0.0.1/fcm/send/abc",
            "https://[::1]/push",
            "https://169.254.169.254/latest/meta-data",
            "https://localhost/push",
            "https://example.com/push",
            "https://fcm.googleapis.com/fcm/send/abc#fragment",
            "https://jmt17.google.com/other",
            "https://evil.jmt17.google.com/fcm/send/abc",
            "not a url",
            "",
        ] {
            assert!(validate(endpoint).is_none(), "{endpoint}");
        }
        let long = format!("https://fcm.googleapis.com/{}", "a".repeat(MAX_LEN));
        assert!(validate(&long).is_none());
    }

    #[test]
    fn the_audience_is_the_push_service_origin() {
        let url = validate("https://wns2-par02p.notify.windows.com/w/?token=BQ").expect("valid");
        assert_eq!(audience(&url), "https://wns2-par02p.notify.windows.com");
        let url = validate("https://web.push.apple.com/QGuQyavXut").expect("valid");
        assert_eq!(audience(&url), "https://web.push.apple.com");
    }
}
