use super::{Selection, http_connect_proxy};
use url::Url;

pub(super) fn select(target: &Url) -> Selection {
    match cccc_system_proxy::macos::for_url(target.as_str()) {
        cccc_system_proxy::Proxy::Direct => Selection::Direct,
        cccc_system_proxy::Proxy::Http { host, port } => http_connect_proxy(&host, port),
        cccc_system_proxy::Proxy::Unavailable => Selection::Unavailable,
    }
}
