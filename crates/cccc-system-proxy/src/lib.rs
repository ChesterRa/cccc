//! Safe, owned results from the platform's existing proxy configuration.
//! Native pointers never escape. Raw configuration must not be logged.
#![deny(unsafe_code)]
#![cfg_attr(not(test), deny(clippy::disallowed_methods))]

/// Filtered macOS result, without credential values or native references.
pub enum Proxy {
    Direct,
    Http { host: String, port: u16 },
    Unavailable,
}

#[cfg(target_os = "macos")]
#[allow(unsafe_code)]
pub mod macos;

#[cfg(windows)]
#[allow(unsafe_code)]
pub mod windows;
