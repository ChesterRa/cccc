use super::{Selector, invalid};
use cccc_contracts::backlog::valid_entry_id;
use serde::Serialize;
use std::{collections::BTreeMap, io, ops::Range};

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct Problem {
    pub code: String,
    pub line: usize,
    pub message: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct Entry {
    pub id: Option<String>,
    pub title: String,
    pub line: usize,
    pub body: String,
    pub problems: Vec<Problem>,
    #[serde(skip)]
    pub(crate) span: Range<usize>,
    #[serde(skip)]
    pub(crate) heading_end: usize,
}

#[derive(Debug, Clone, Serialize)]
pub struct Section {
    pub name: String,
    pub line: usize,
    pub closed: bool,
    pub entries: Vec<Entry>,
    #[serde(skip)]
    pub(crate) span: Range<usize>,
}

#[derive(Debug, Clone)]
pub struct Document {
    pub sections: Vec<Section>,
    pub problems: Vec<Problem>,
    pub(crate) raw: String,
}

pub(crate) fn problem(code: &str, line: usize, message: &str) -> Problem {
    Problem {
        code: code.into(),
        line,
        message: message.into(),
    }
}

impl Document {
    pub fn parse(text: &str) -> Self {
        let mut document = Self {
            sections: vec![Section {
                name: String::new(),
                line: 0,
                closed: false,
                entries: Vec::new(),
                span: 0..text.len(),
            }],
            problems: Vec::new(),
            raw: text.into(),
        };
        let mut fence: Option<(u8, usize, usize)> = None;
        let mut offset = 0;
        for (index, raw_line) in text.split_inclusive('\n').enumerate() {
            let line = raw_line.trim_end_matches(['\r', '\n']);
            let line_number = index + 1;
            if let Some((kind, length, _)) = fence {
                if let Some((found, count, rest)) = fence_marker(line)
                    && found == kind
                    && count >= length
                    && rest.trim().is_empty()
                {
                    fence = None;
                }
            } else if let Some((kind, count, rest)) = fence_marker(line)
                && (kind != b'`' || !rest.contains('`'))
            {
                fence = Some((kind, count, line_number));
            } else if let Some(name) = line.strip_prefix("## ") {
                document.finish_entry(offset);
                if let Some(section) = document.sections.last_mut() {
                    section.span.end = offset;
                }
                let name = name.trim().to_owned();
                document.sections.push(Section {
                    closed: name == "Closed",
                    name,
                    line: line_number,
                    entries: Vec::new(),
                    span: offset..text.len(),
                });
            } else if let Some(heading) = line.strip_prefix("### ") {
                document.finish_entry(offset);
                let (title, id, malformed) = parse_title(heading);
                let mut problems = Vec::new();
                if malformed {
                    problems.push(problem(
                        "malformed_id",
                        line_number,
                        "Malformed entry ID; fix its heading before changing it",
                    ));
                } else if id.is_none() {
                    problems.push(problem(
                        "missing_id",
                        line_number,
                        "Entry has no stable ID; an explicit write can add one",
                    ));
                }
                if title.is_empty() {
                    problems.push(problem("empty_title", line_number, "Entry title is empty"));
                }
                document
                    .sections
                    .last_mut()
                    .expect("section")
                    .entries
                    .push(Entry {
                        id,
                        title,
                        line: line_number,
                        body: String::new(),
                        problems,
                        span: offset..text.len(),
                        heading_end: offset + raw_line.len(),
                    });
            }
            offset += raw_line.len();
        }
        document.finish_entry(text.len());
        if let Some((_, _, line)) = fence {
            document.problems.push(problem(
                "unterminated_fence",
                line,
                "Close the code fence before editing entries",
            ));
        }
        let mut ids = BTreeMap::<String, usize>::new();
        let mut names = BTreeMap::<String, usize>::new();
        for section in &document.sections {
            *names.entry(section.name.clone()).or_default() += 1;
            for entry in &section.entries {
                if let Some(id) = &entry.id {
                    *ids.entry(id.clone()).or_default() += 1;
                }
            }
        }
        for section in &mut document.sections {
            if names[&section.name] > 1 {
                document.problems.push(problem(
                    "duplicate_section",
                    section.line,
                    "Section name appears more than once",
                ));
            }
            for entry in &mut section.entries {
                if entry.id.as_ref().is_some_and(|id| ids[id] > 1) {
                    entry.problems.push(problem(
                        "duplicate_id",
                        entry.line,
                        "Entry ID appears more than once",
                    ));
                }
                document.problems.extend(entry.problems.clone());
            }
        }
        document
    }

    fn finish_entry(&mut self, end: usize) {
        if let Some(entry) = self
            .sections
            .last_mut()
            .and_then(|section| section.entries.last_mut())
        {
            entry.span.end = end;
            entry.body = self.raw[entry.heading_end..end].to_owned();
        }
    }

    pub fn content(&self) -> &str {
        &self.raw
    }

    pub(crate) fn writable(&self) -> io::Result<()> {
        if self
            .problems
            .iter()
            .any(|problem| problem.code == "unterminated_fence")
        {
            return Err(invalid("unterminated_fence"));
        }
        Ok(())
    }

    pub(crate) fn entry(&self, selector: &Selector) -> io::Result<&Entry> {
        self.writable()?;
        let mut matching = self
            .sections
            .iter()
            .flat_map(|section| &section.entries)
            .filter(|entry| match selector {
                Selector::Id(id) => entry.id.as_ref() == Some(id),
                Selector::Line(line) => entry.id.is_none() && entry.line == *line,
            });
        let entry = matching.next().ok_or_else(|| invalid("entry_not_found"))?;
        if matching.next().is_some() {
            return Err(invalid("ambiguous_entry"));
        }
        if entry.problems.iter().any(|p| p.code != "missing_id") {
            return Err(invalid("entry_problem"));
        }
        Ok(entry)
    }

    pub(crate) fn section(&self, name: &str) -> io::Result<&Section> {
        self.writable()?;
        let mut matching = self.sections.iter().filter(|section| section.name == name);
        let section = matching
            .next()
            .ok_or_else(|| invalid("section_not_found"))?;
        if matching.next().is_some() {
            return Err(invalid("ambiguous_section"));
        }
        Ok(section)
    }

    pub(crate) fn newline(&self) -> &str {
        if self
            .raw
            .find('\n')
            .is_some_and(|i| i > 0 && self.raw.as_bytes()[i - 1] == b'\r')
        {
            "\r\n"
        } else {
            "\n"
        }
    }

    pub(crate) fn new_id(&self) -> String {
        loop {
            let id = uuid::Uuid::new_v4().simple().to_string()[..12].to_owned();
            if !self
                .sections
                .iter()
                .flat_map(|section| &section.entries)
                .any(|entry| entry.id.as_ref() == Some(&id))
            {
                return id;
            }
        }
    }
}

fn parse_title(heading: &str) -> (String, Option<String>, bool) {
    let heading = heading.trim();
    if let Some((title, suffix)) = heading.rsplit_once("<!-- id:") {
        if let Some(id) = suffix.trim().strip_suffix("-->").map(str::trim)
            && valid_entry_id(id)
            && !title.contains("<!-- id:")
        {
            return (title.trim().into(), Some(id.into()), false);
        }
        return (heading.into(), None, true);
    }
    (heading.into(), None, false)
}

fn fence_marker(line: &str) -> Option<(u8, usize, &str)> {
    let trimmed = line.trim_start_matches(' ');
    if line.len() - trimmed.len() > 3 {
        return None;
    }
    let kind = *trimmed.as_bytes().first()?;
    if kind != b'`' && kind != b'~' {
        return None;
    }
    let count = trimmed.bytes().take_while(|byte| *byte == kind).count();
    (count >= 3).then_some((kind, count, &trimmed[count..]))
}
