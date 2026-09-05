//! Asking the operator before stopping something that is already running.
//!
//! Every "should I stop the process that is already here?" decision shares one
//! safety rule: without a terminal to ask, the answer is no. A launcher started
//! by an MCP bridge, a service manager, or CI has nobody to consult, and
//! guessing there would terminate processes on behalf of an operator who never
//! saw the question. Keeping the rule in one place is what keeps the two call
//! sites -- the Web instance lock and the daemon lock -- from drifting apart.

use anyhow::Result;
use std::io::{self, BufRead, IsTerminal, Write};

/// Ask a yes/no question, defaulting to no.
///
/// Returns `false` without prompting when stdin is not a terminal.
pub(crate) fn ask(question: &str) -> Result<bool> {
    if !io::stdin().is_terminal() {
        return Ok(false);
    }
    ask_with(question, &mut io::stdin().lock(), &mut io::stderr().lock())
}

pub(crate) fn ask_with(
    question: &str,
    input: &mut impl BufRead,
    output: &mut impl Write,
) -> Result<bool> {
    write!(output, "{question} [y/N] ")?;
    output.flush()?;

    let mut answer = String::new();
    input.read_line(&mut answer)?;
    Ok(matches!(
        answer.trim().to_ascii_lowercase().as_str(),
        "y" | "yes"
    ))
}

#[cfg(test)]
mod tests {
    use super::ask_with;

    #[test]
    fn accepts_only_explicit_yes() {
        let mut output = Vec::new();
        assert!(ask_with("Stop it?", &mut "yes\n".as_bytes(), &mut output).expect("yes"));
        assert!(ask_with("Stop it?", &mut "y\n".as_bytes(), &mut output).expect("y"));
        assert!(!ask_with("Stop it?", &mut "\n".as_bytes(), &mut output).expect("default no"));
        assert!(!ask_with("Stop it?", &mut "n\n".as_bytes(), &mut output).expect("no"));
        // Anything that is not an explicit yes must not be read as consent.
        assert!(!ask_with("Stop it?", &mut "sure\n".as_bytes(), &mut output).expect("not yes"));
    }

    #[test]
    fn eof_is_not_consent() {
        let mut output = Vec::new();
        assert!(!ask_with("Stop it?", &mut "".as_bytes(), &mut output).expect("eof"));
    }
}
