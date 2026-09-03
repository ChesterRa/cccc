use windows_sys::Win32::System::Console::{
    GetConsoleCP, GetConsoleOutputCP, SetConsoleCP, SetConsoleOutputCP,
};

const UTF8_CODE_PAGE: u32 = 65001;

pub(crate) struct ConsoleEncoding {
    input: Option<u32>,
    output: Option<u32>,
}

pub(crate) fn use_utf8() -> ConsoleEncoding {
    let input = unsafe { GetConsoleCP() };
    let output = unsafe { GetConsoleOutputCP() };
    let input =
        (input != 0 && input != UTF8_CODE_PAGE && unsafe { SetConsoleCP(UTF8_CODE_PAGE) } != 0)
            .then_some(input);
    let output = (output != 0
        && output != UTF8_CODE_PAGE
        && unsafe { SetConsoleOutputCP(UTF8_CODE_PAGE) } != 0)
        .then_some(output);
    ConsoleEncoding { input, output }
}

impl Drop for ConsoleEncoding {
    fn drop(&mut self) {
        if let Some(code_page) = self.input {
            unsafe { SetConsoleCP(code_page) };
        }
        if let Some(code_page) = self.output {
            unsafe { SetConsoleOutputCP(code_page) };
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use windows_sys::Win32::System::Console::{AllocConsole, FreeConsole};

    const TEST_CODE_PAGE: u32 = 437;

    struct TestConsole {
        original: (u32, u32),
        allocated: bool,
    }

    impl TestConsole {
        fn attach() -> Self {
            let original = unsafe { (GetConsoleCP(), GetConsoleOutputCP()) };
            let allocated = original.0 == 0 || original.1 == 0;
            if allocated {
                assert_ne!(
                    unsafe { AllocConsole() },
                    0,
                    "test runner has no console and AllocConsole failed"
                );
            }
            assert_ne!(unsafe { GetConsoleCP() }, 0, "input console unavailable");
            assert_ne!(
                unsafe { GetConsoleOutputCP() },
                0,
                "output console unavailable"
            );
            Self {
                original,
                allocated,
            }
        }
    }

    impl Drop for TestConsole {
        fn drop(&mut self) {
            if self.allocated {
                unsafe { FreeConsole() };
            } else {
                unsafe {
                    SetConsoleCP(self.original.0);
                    SetConsoleOutputCP(self.original.1);
                }
            }
        }
    }

    #[test]
    fn console_uses_utf8_for_cli_lifetime_and_restores_previous_pages() {
        let console = TestConsole::attach();
        assert_ne!(unsafe { SetConsoleCP(TEST_CODE_PAGE) }, 0);
        assert_ne!(unsafe { SetConsoleOutputCP(TEST_CODE_PAGE) }, 0);
        assert_eq!(
            unsafe { (GetConsoleCP(), GetConsoleOutputCP()) },
            (TEST_CODE_PAGE, TEST_CODE_PAGE)
        );

        {
            let _guard = use_utf8();
            assert_eq!(
                unsafe { (GetConsoleCP(), GetConsoleOutputCP()) },
                (UTF8_CODE_PAGE, UTF8_CODE_PAGE)
            );
        }
        assert_eq!(
            unsafe { (GetConsoleCP(), GetConsoleOutputCP()) },
            (TEST_CODE_PAGE, TEST_CODE_PAGE)
        );

        let original = console.original;
        let allocated = console.allocated;
        drop(console);
        if !allocated {
            assert_eq!(unsafe { (GetConsoleCP(), GetConsoleOutputCP()) }, original);
        }
    }
}
