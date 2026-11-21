//! Current user's Internet Options for the active connection (LAN, dial-up, or VPN).
use windows_sys::Win32::Foundation::{ERROR_FILE_NOT_FOUND, GetLastError, GlobalFree};
use windows_sys::Win32::Networking::WinHttp::{
    WINHTTP_CURRENT_USER_IE_PROXY_CONFIG, WinHttpGetIEProxyConfigForCurrentUser,
};

/// Static proxy strings can contain user information; never put them in diagnostics.
pub enum Configuration {
    Direct,
    Manual { server: String, bypass: String },
    Unavailable,
}

#[derive(Default)]
struct Settings(WINHTTP_CURRENT_USER_IE_PROXY_CONFIG);

impl Drop for Settings {
    fn drop(&mut self) {
        for ptr in [
            self.0.lpszAutoConfigUrl,
            self.0.lpszProxy,
            self.0.lpszProxyBypass,
        ] {
            if !ptr.is_null() {
                // SAFETY: WinHTTP transfers a separate GlobalAlloc allocation for each field.
                unsafe { GlobalFree(ptr.cast()) };
            }
        }
    }
}

/// Read a WinHTTP-owned NUL-terminated UTF-16 string, retaining no native pointers.
/// A null pointer is an absent setting; malformed UTF-16 is an unavailable setting.
unsafe fn text(ptr: *const u16) -> Option<String> {
    if ptr.is_null() {
        return Some(String::new());
    }
    // SAFETY: the caller holds the Settings that owns this NUL-terminated string.
    unsafe {
        let mut len = 0;
        while *ptr.add(len) != 0 {
            len += 1;
        }
        String::from_utf16(std::slice::from_raw_parts(ptr, len)).ok()
    }
}

pub fn current_user() -> Configuration {
    let mut settings = Settings::default();
    // SAFETY: the output structure is initialized and its allocations are owned by settings.
    if unsafe { WinHttpGetIEProxyConfigForCurrentUser(&mut settings.0) } == 0 {
        // SAFETY: read this thread's WinHTTP error before dropping the output structure.
        return if unsafe { GetLastError() } == ERROR_FILE_NOT_FOUND {
            Configuration::Direct
        } else {
            Configuration::Unavailable
        };
    }
    let raw = &settings.0;
    // SAFETY: after a successful WinHTTP call this is null or a valid NUL-terminated string.
    let has_pac = unsafe { raw.lpszAutoConfigUrl.as_ref() }.is_some_and(|first| *first != 0);
    if raw.fAutoDetect != 0 || has_pac {
        return Configuration::Unavailable;
    }
    // SAFETY: both strings remain allocated until settings drops at function exit.
    let (server, bypass) = unsafe { (text(raw.lpszProxy), text(raw.lpszProxyBypass)) };
    match (server, bypass) {
        (Some(server), _) if server.trim().is_empty() => Configuration::Direct,
        (Some(server), Some(bypass)) => Configuration::Manual { server, bypass },
        _ => Configuration::Unavailable,
    }
}
