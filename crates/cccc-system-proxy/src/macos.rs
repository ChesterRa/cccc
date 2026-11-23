//! CFNetwork policy lookup only: no PAC execution, Keychain access or watchers.
use crate::Proxy;
use core_foundation::{
    array::{CFArray, CFArrayRef},
    base::{CFType, TCFType},
    boolean::CFBoolean,
    dictionary::{CFDictionary, CFDictionaryRef},
    number::CFNumber,
    string::{CFString, CFStringRef},
    url::{CFURL, CFURLCreateWithString, CFURLRef},
};

type Settings = CFDictionary<CFString, CFType>;

#[link(name = "CFNetwork", kind = "framework")]
unsafe extern "C" {
    fn CFNetworkCopySystemProxySettings() -> CFDictionaryRef;
    fn CFNetworkCopyProxiesForURL(url: CFURLRef, settings: CFDictionaryRef) -> CFArrayRef;
    static kCFProxyTypeKey: CFStringRef;
    static kCFProxyTypeNone: CFStringRef;
    static kCFProxyTypeHTTP: CFStringRef;
    static kCFProxyTypeHTTPS: CFStringRef;
    static kCFProxyHostNameKey: CFStringRef;
    static kCFProxyPortNumberKey: CFStringRef;
    static kCFProxyUsernameKey: CFStringRef;
    static kCFProxyPasswordKey: CFStringRef;
}

/// Read one system snapshot and resolve this destination without network traffic.
pub fn for_url(target: &str) -> Proxy {
    // SAFETY: Copy returns an owned dictionary of CFString keys/CFType values.
    // A null result is ambiguous (no settings or failure), so fail closed.
    let settings: Settings = unsafe {
        let raw = CFNetworkCopySystemProxySettings();
        if raw.is_null() {
            return Proxy::Unavailable;
        }
        Settings::wrap_under_create_rule(raw)
    };
    resolve(target, &settings)
}

fn resolve(target: &str, settings: &Settings) -> Proxy {
    // WPAD can otherwise appear as None or even a manual proxy. Neither proves
    // that an unimplemented automatic policy permits direct/static access.
    if enabled(settings, "ProxyAutoConfigEnable") || enabled(settings, "ProxyAutoDiscoveryEnable") {
        return Proxy::Unavailable;
    }
    let text = CFString::new(target);
    // SAFETY: input CFString lives through the call; Create ownership belongs
    // to CFURL. No PAC execution functions are linked or called here.
    let url: CFURL = unsafe {
        let raw = CFURLCreateWithString(
            std::ptr::null(),
            text.as_concrete_TypeRef(),
            std::ptr::null(),
        );
        if raw.is_null() {
            return Proxy::Unavailable;
        }
        CFURL::wrap_under_create_rule(raw)
    };
    // SAFETY: both inputs are live CF objects. The documented Copy result is
    // an owned array of proxy dictionaries; each dictionary has CFString keys.
    let proxies: CFArray<Settings> = unsafe {
        let raw =
            CFNetworkCopyProxiesForURL(url.as_concrete_TypeRef(), settings.as_concrete_TypeRef());
        if raw.is_null() {
            return Proxy::Unavailable;
        }
        CFArray::wrap_under_create_rule(raw)
    };
    match proxies.get(0) {
        Some(first) => classify(&first, settings),
        None => Proxy::Unavailable,
    }
}

fn field(settings: &Settings, key: &CFString) -> Option<CFType> {
    settings.find(key).map(|value| (*value).clone())
}

fn enabled(settings: &Settings, key: &str) -> bool {
    let Some(value) = field(settings, &CFString::new(key)) else {
        return false;
    };
    if let Some(number) = value.downcast::<CFNumber>() {
        return number.to_i64().is_none_or(|value| value != 0);
    }
    value
        .downcast::<CFBoolean>()
        .map(bool::from)
        .unwrap_or(true)
}

fn classify(proxy: &Settings, settings: &Settings) -> Proxy {
    // SAFETY: these immutable CFNetwork constants are valid for the lifetime
    // of the linked framework. Get-rule wrapping retains each borrowed value.
    let (kind_key, none, http, https, host_key, port_key, user_key, password_key) = unsafe {
        (
            CFString::wrap_under_get_rule(kCFProxyTypeKey),
            CFString::wrap_under_get_rule(kCFProxyTypeNone),
            CFString::wrap_under_get_rule(kCFProxyTypeHTTP),
            CFString::wrap_under_get_rule(kCFProxyTypeHTTPS),
            CFString::wrap_under_get_rule(kCFProxyHostNameKey),
            CFString::wrap_under_get_rule(kCFProxyPortNumberKey),
            CFString::wrap_under_get_rule(kCFProxyUsernameKey),
            CFString::wrap_under_get_rule(kCFProxyPasswordKey),
        )
    };
    let Some(kind) = field(proxy, &kind_key).and_then(|value| value.downcast::<CFString>()) else {
        return Proxy::Unavailable;
    };
    if kind == none {
        return Proxy::Direct;
    }
    let prefix = if kind == http {
        "HTTP"
    } else if kind == https {
        "HTTPS"
    } else {
        return Proxy::Unavailable;
    };
    // CFNetwork may omit these from the per-URL item even when the original
    // dictionary exposes them. Never ask external credential stores for more.
    if proxy.contains_key(&user_key)
        || proxy.contains_key(&password_key)
        || settings.contains_key(&CFString::new(&format!("{prefix}User")))
        || settings.contains_key(&CFString::new(&format!("{prefix}Password")))
    {
        return Proxy::Unavailable;
    }
    let Some(host) = field(proxy, &host_key)
        .and_then(|value| value.downcast::<CFString>())
        .map(|value| value.to_string())
    else {
        return Proxy::Unavailable;
    };
    let port = field(proxy, &port_key)
        .and_then(|value| value.downcast::<CFNumber>())
        .and_then(|value| value.to_i64())
        .and_then(|value| u16::try_from(value).ok());
    match port {
        Some(port) if port != 0 && !host.is_empty() => Proxy::Http { host, port },
        _ => Proxy::Unavailable,
    }
}

#[cfg(test)]
#[path = "macos_tests.rs"]
mod tests;
