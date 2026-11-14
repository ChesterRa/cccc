//! Reachable Direct endpoint address validation.
use super::*;

pub fn address(value: &str) -> io::Result<String> {
    let url = url::Url::parse(&format!("tcp://{value}"))
        .map_err(|_| invalid("Use a hostname or IP address with a port"))?;
    if url.host_str().is_none()
        || url
            .host_str()
            .and_then(|host| {
                host.trim_matches(['[', ']'])
                    .parse::<std::net::IpAddr>()
                    .ok()
            })
            .is_some_and(|ip| ip.is_unspecified())
        || url.port().is_none_or(|port| port == 0)
        || !url.username().is_empty()
        || url.password().is_some()
        || !url.path().is_empty()
        || url.query().is_some()
        || url.fragment().is_some()
    {
        return Err(invalid(
            "Use a reachable hostname or IP address with a port, not 0.0.0.0 or ::",
        ));
    }
    Ok(value.to_owned())
}
