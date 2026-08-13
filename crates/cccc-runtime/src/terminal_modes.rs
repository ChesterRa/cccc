const MAX_KEYBOARD_STACK_DEPTH: usize = 32;

#[derive(Debug, Default)]
struct KeyboardMode {
    flags: u32,
    stack: Vec<u32>,
}

#[derive(Debug, Default)]
pub(crate) struct TerminalModes {
    alternate_screen: bool,
    main_keyboard: KeyboardMode,
    alternate_keyboard: KeyboardMode,
}

impl TerminalModes {
    pub(crate) const fn alternate_screen(&self) -> bool {
        self.alternate_screen
    }

    pub(crate) fn main_keyboard_restore(&self) -> Vec<u8> {
        self.main_keyboard.restore()
    }

    pub(crate) fn active_keyboard_restore(&self) -> Vec<u8> {
        if self.alternate_screen {
            &self.alternate_keyboard
        } else {
            &self.main_keyboard
        }
        .restore()
    }

    pub(crate) fn apply_csi(&mut self, sequence: &[u8]) {
        let Some(body) = sequence
            .strip_prefix(b"\x1b[")
            .or_else(|| sequence.strip_prefix(&[0x9b]))
        else {
            return;
        };
        let Some((&final_byte, params)) = body.split_last() else {
            return;
        };
        if matches!(final_byte, b'h' | b'l') && params.first() == Some(&b'?') {
            let enabled = final_byte == b'h';
            for value in params[1..].split(|byte| *byte == b';') {
                if matches!(parse_u32(value), Some(47 | 1047 | 1049)) {
                    self.alternate_screen = enabled;
                }
            }
            return;
        }
        if final_byte != b'u' {
            return;
        }
        if self.alternate_screen {
            &mut self.alternate_keyboard
        } else {
            &mut self.main_keyboard
        }
        .apply(params);
    }
}

impl KeyboardMode {
    fn apply(&mut self, params: &[u8]) {
        match params.first().copied() {
            Some(b'=') => {
                let mut values = params[1..].split(|byte| *byte == b';');
                let flags = values.next().and_then(parse_u32).unwrap_or(0);
                match values.next().and_then(parse_u32).unwrap_or(1) {
                    2 => self.flags |= flags,
                    3 => self.flags &= !flags,
                    _ => self.flags = flags,
                }
            }
            Some(b'>') => {
                if self.stack.len() == MAX_KEYBOARD_STACK_DEPTH {
                    self.stack.remove(0);
                }
                self.stack.push(self.flags);
                self.flags = parse_u32(&params[1..]).unwrap_or(0);
            }
            Some(b'<') => {
                let count = parse_u32(&params[1..]).unwrap_or(1).min(64);
                for _ in 0..count {
                    self.flags = self.stack.pop().unwrap_or(0);
                }
            }
            _ => {}
        }
    }

    fn restore(&self) -> Vec<u8> {
        if self.flags == 0 && self.stack.is_empty() {
            return Vec::new();
        }
        let mut output = Vec::new();
        if self.stack.is_empty() {
            append_set(&mut output, self.flags);
            return output;
        }
        append_set(&mut output, self.stack[0]);
        for flags in self.stack.iter().skip(1).copied().chain([self.flags]) {
            output.extend_from_slice(b"\x1b[>");
            output.extend_from_slice(flags.to_string().as_bytes());
            output.push(b'u');
        }
        output
    }
}

fn parse_u32(value: &[u8]) -> Option<u32> {
    std::str::from_utf8(value).ok()?.parse().ok()
}

fn append_set(output: &mut Vec<u8>, flags: u32) {
    output.extend_from_slice(b"\x1b[=");
    output.extend_from_slice(flags.to_string().as_bytes());
    output.extend_from_slice(b";1u");
}
