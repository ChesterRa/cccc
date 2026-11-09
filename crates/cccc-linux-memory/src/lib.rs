#![deny(unsafe_code)]

#[cfg(all(target_os = "linux", target_env = "gnu"))]
#[allow(unsafe_code)]
mod sys {
    unsafe extern "C" {
        fn malloc_trim(pad: usize) -> std::ffi::c_int;
    }

    /// Ask glibc to return unused pages, reporting whether it released memory.
    /// This does not guarantee a particular change in process RSS.
    pub fn trim_unused_pages() -> bool {
        // SAFETY: glibc's malloc_trim takes only an integer, not caller-owned
        // memory. It locks each arena and is safe in ordinary threads. This
        // function must not be used from an asynchronous signal handler.
        unsafe { malloc_trim(0) != 0 }
    }
}

#[cfg(all(target_os = "linux", target_env = "gnu"))]
pub use sys::trim_unused_pages;

#[cfg(all(test, target_os = "linux", target_env = "gnu"))]
mod tests {
    #[test]
    fn trimming_preserves_live_allocations() {
        let held = vec![37_u8; 64 * 1024];
        let _released = super::trim_unused_pages();
        assert!(held.iter().all(|byte| *byte == 37));
    }
}
