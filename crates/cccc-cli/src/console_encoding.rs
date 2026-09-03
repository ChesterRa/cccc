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

    const GBK_CODE_PAGE: u32 = 936;

    struct RestoreConsoleCodePages {
        input: u32,
        output: u32,
    }

    impl Drop for RestoreConsoleCodePages {
        fn drop(&mut self) {
            unsafe {
                SetConsoleCP(self.input);
                SetConsoleOutputCP(self.output);
            }
        }
    }

    #[test]
    fn console_uses_utf8_for_cli_lifetime_and_restores_previous_pages() {
        let runner = unsafe { (GetConsoleCP(), GetConsoleOutputCP()) };
        let restore_runner = RestoreConsoleCodePages {
            input: runner.0,
            output: runner.1,
        };
        assert_ne!(runner.0, 0, "test requires an attached input console");
        assert_ne!(runner.1, 0, "test requires an attached output console");
        assert_ne!(unsafe { SetConsoleCP(GBK_CODE_PAGE) }, 0);
        assert_ne!(unsafe { SetConsoleOutputCP(GBK_CODE_PAGE) }, 0);
        assert_eq!(
            unsafe { (GetConsoleCP(), GetConsoleOutputCP()) },
            (GBK_CODE_PAGE, GBK_CODE_PAGE)
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
            (GBK_CODE_PAGE, GBK_CODE_PAGE)
        );

        drop(restore_runner);
        assert_eq!(unsafe { (GetConsoleCP(), GetConsoleOutputCP()) }, runner);
    }
}
