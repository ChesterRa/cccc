#![deny(unsafe_code)]

#[cfg(windows)]
#[allow(unsafe_code)]
mod identity;
#[cfg(windows)]
pub use identity::{creation_time, image_path, parent_pid};

#[cfg(windows)]
#[allow(unsafe_code)]
mod suspended;
#[cfg(windows)]
pub use suspended::{SuspendedChild, spawn_suspended, spawn_suspended_with_creation_observer};

#[cfg(all(test, windows))]
mod tests;
