pub(crate) fn format(host: &str, port: u16) -> String {
    let host = match host {
        "0.0.0.0" | "::" => "127.0.0.1",
        value => value,
    };
    let url_host = if host.starts_with('[') && host.ends_with(']') {
        host.to_owned()
    } else if host.contains(':') {
        format!("[{host}]")
    } else {
        host.to_owned()
    };
    format!("http://{url_host}:{port}")
}

#[cfg(test)]
mod tests {
    use super::format;

    #[test]
    fn brackets_ipv6_literals() {
        assert_eq!(format("::1", 8848), "http://[::1]:8848");
        assert_eq!(format("[2001:db8::1]", 9000), "http://[2001:db8::1]:9000");
        assert_eq!(format("127.0.0.1", 8848), "http://127.0.0.1:8848");
    }
}
