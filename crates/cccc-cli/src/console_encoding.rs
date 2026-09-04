const UTF8_CODE_PAGE: u32 = 65001;

pub(crate) struct ConsoleEncoding {
    original_input: Option<u32>,
    original_output: Option<u32>,
}

pub(crate) fn use_utf8() -> ConsoleEncoding {
    let original_input = cccc_windows_console::input_code_page().filter(|code_page| {
        *code_page != UTF8_CODE_PAGE && cccc_windows_console::set_input_code_page(UTF8_CODE_PAGE)
    });
    let original_output = cccc_windows_console::output_code_page().filter(|code_page| {
        *code_page != UTF8_CODE_PAGE && cccc_windows_console::set_output_code_page(UTF8_CODE_PAGE)
    });
    ConsoleEncoding {
        original_input,
        original_output,
    }
}

impl Drop for ConsoleEncoding {
    fn drop(&mut self) {
        if let Some(code_page) = self.original_input {
            let _ = cccc_windows_console::set_input_code_page(code_page);
        }
        if let Some(code_page) = self.original_output {
            let _ = cccc_windows_console::set_output_code_page(code_page);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct RestoreCodePages {
        input: u32,
        output: u32,
    }

    impl Drop for RestoreCodePages {
        fn drop(&mut self) {
            let _ = cccc_windows_console::set_input_code_page(self.input);
            let _ = cccc_windows_console::set_output_code_page(self.output);
        }
    }

    #[test]
    fn console_uses_utf8_for_cli_lifetime_and_restores_both_original_pages() {
        let (Some(input), Some(output)) = (
            cccc_windows_console::input_code_page(),
            cccc_windows_console::output_code_page(),
        ) else {
            return;
        };
        let _restore = RestoreCodePages { input, output };
        let test_input = if input == UTF8_CODE_PAGE { 437 } else { input };
        let test_output = if output == UTF8_CODE_PAGE {
            932
        } else {
            output
        };
        assert!(cccc_windows_console::set_input_code_page(test_input));
        assert!(cccc_windows_console::set_output_code_page(test_output));

        {
            let _guard = use_utf8();
            assert_eq!(
                cccc_windows_console::input_code_page(),
                Some(UTF8_CODE_PAGE)
            );
            assert_eq!(
                cccc_windows_console::output_code_page(),
                Some(UTF8_CODE_PAGE)
            );
        }
        assert_eq!(cccc_windows_console::input_code_page(), Some(test_input));
        assert_eq!(cccc_windows_console::output_code_page(), Some(test_output));
    }
}
