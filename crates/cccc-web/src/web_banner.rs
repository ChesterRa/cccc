use std::net::Ipv4Addr;

use crate::network::detect_lan_ipv4;

pub fn print(host: &str, port: u16) {
    let (local_url, network_url) = urls(host, port, detect_lan_ipv4());
    eprintln!("[cccc] Implementation: rust");
    eprintln!("[cccc] Starting web server...");
    eprintln!("[cccc]   Local:   {local_url}");
    if let Some(network_url) = network_url {
        eprintln!("[cccc]   Network: {network_url}");
    }
}

fn urls(host: &str, port: u16, lan_ip: Option<Ipv4Addr>) -> (String, Option<String>) {
    let host = host.trim();
    let local_host = match host {
        "" | "0.0.0.0" | "::" | "[::]" => "localhost".to_owned(),
        value if value.contains(':') && !(value.starts_with('[') && value.ends_with(']')) => {
            format!("[{value}]")
        }
        value => value.to_owned(),
    };
    let local_url = format!("http://{local_host}:{port}");
    let network_url = lan_ip
        .filter(|ip| host != ip.to_string())
        .map(|ip| format!("http://{ip}:{port}"));
    (local_url, network_url)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wildcard_binding_shows_localhost_and_lan_urls() {
        assert_eq!(
            urls("0.0.0.0", 8848, Some(Ipv4Addr::new(192, 168, 1, 20))),
            (
                "http://localhost:8848".into(),
                Some("http://192.168.1.20:8848".into())
            )
        );
    }

    #[test]
    fn ipv6_literal_is_bracketed() {
        assert_eq!(urls("::1", 9000, None), ("http://[::1]:9000".into(), None));
    }
}
