#[path = "suite/daemon_self_launch.rs"]
mod daemon_self_launch;

#[path = "suite/kimi_setup.rs"]
mod kimi_setup;

#[cfg(unix)]
#[path = "suite/grok_setup.rs"]
mod grok_setup;

// This offline installed-distribution fixture is pinned to Linux x86_64.
#[cfg(all(target_os = "linux", target_arch = "x86_64"))]
#[path = "suite/antigravity_setup.rs"]
mod antigravity_setup;
