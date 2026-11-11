//! Byte-preserving sections and entries in a user-owned Markdown file.
mod edit;
mod parser;
#[cfg(test)]
mod tests;

pub use parser::{Document, Entry, Problem, Section};

use std::{io, ops::Range};

pub(crate) fn invalid(message: impl Into<String>) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidInput, message.into())
}

#[derive(Debug, Clone)]
pub enum Selector {
    Id(String),
    Line(usize),
}

pub struct Edit {
    pub content: String,
    pub entry_id: String,
}

fn replace(text: &str, range: Range<usize>, replacement: &str) -> String {
    let mut result = String::with_capacity(text.len() + replacement.len());
    result.push_str(&text[..range.start]);
    result.push_str(replacement);
    result.push_str(&text[range.end..]);
    result
}
