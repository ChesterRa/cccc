#![deny(unsafe_code)]

#[cfg(windows)]
#[allow(unsafe_code)]
mod sys {
    use windows_sys::Win32::System::Console::{
        GetConsoleCP, GetConsoleOutputCP, SetConsoleCP, SetConsoleOutputCP,
    };

    pub fn input_code_page() -> Option<u32> {
        // SAFETY: GetConsoleCP has no pointer arguments. Zero means that the
        // process has no console or the Win32 call failed.
        let value = unsafe { GetConsoleCP() };
        (value != 0).then_some(value)
    }

    pub fn output_code_page() -> Option<u32> {
        // SAFETY: GetConsoleOutputCP has no pointer arguments. Zero means that
        // the process has no console or the Win32 call failed.
        let value = unsafe { GetConsoleOutputCP() };
        (value != 0).then_some(value)
    }

    pub fn set_input_code_page(value: u32) -> bool {
        // SAFETY: SetConsoleCP accepts a numeric code-page identifier and does
        // not dereference caller-provided memory.
        unsafe { SetConsoleCP(value) != 0 }
    }

    pub fn set_output_code_page(value: u32) -> bool {
        // SAFETY: SetConsoleOutputCP accepts a numeric code-page identifier and
        // does not dereference caller-provided memory.
        unsafe { SetConsoleOutputCP(value) != 0 }
    }
}

#[cfg(windows)]
pub use sys::{input_code_page, output_code_page, set_input_code_page, set_output_code_page};
