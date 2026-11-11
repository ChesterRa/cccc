use super::{Document, Edit, Selector, invalid, replace};
use std::io;

impl Document {
    pub fn add(&self, section: &str, title: &str, body: &str) -> io::Result<Edit> {
        if section == "Closed" {
            return Err(invalid("closed_target"));
        }
        let target = self.section(section)?;
        let id = self.new_id();
        let block = block(title, body, &id, self.newline())?;
        Ok(Edit {
            content: insert(&self.raw, target.span.end, &block, self.newline()),
            entry_id: id,
        })
    }

    pub fn identify(&self, selector: &Selector) -> io::Result<Edit> {
        let entry = self.entry(selector)?;
        if let Some(id) = &entry.id {
            return Ok(Edit {
                content: self.raw.clone(),
                entry_id: id.clone(),
            });
        }
        let id = self.new_id();
        let raw = &self.raw[entry.span.start..entry.heading_end];
        let end = raw.len()
            - if raw.ends_with("\r\n") {
                2
            } else {
                usize::from(raw.ends_with('\n'))
            };
        // Identification adds metadata without normalizing the user's heading,
        // including its whitespace or its own line ending in a mixed file.
        let heading = format!("{} <!-- id: {id} -->{}", &raw[..end], &raw[end..]);
        Ok(Edit {
            content: replace(&self.raw, entry.span.start..entry.heading_end, &heading),
            entry_id: id,
        })
    }

    pub fn edit(&self, selector: &Selector, title: &str, body: &str) -> io::Result<Edit> {
        let entry = self.entry(selector)?;
        let id = entry.id.clone().unwrap_or_else(|| self.new_id());
        let mut block = block(title, body, &id, self.newline())?;
        if entry.span.end < self.raw.len() {
            block.push_str(&padding(&block, self.newline()));
        }
        Ok(Edit {
            content: replace(&self.raw, entry.span.clone(), &block),
            entry_id: id,
        })
    }

    pub fn move_entry(&self, selector: &Selector, section: &str) -> io::Result<Edit> {
        if section == "Closed" {
            return Err(invalid("closed_target"));
        }
        self.section(section)?;
        let identified = self.identify(selector)?;
        let document = Document::parse(&identified.content);
        let entry = document.entry(&Selector::Id(identified.entry_id.clone()))?;
        if document
            .section(section)?
            .entries
            .last()
            .is_some_and(|last| last.id == entry.id)
        {
            return Ok(identified);
        }
        let block = document.raw[entry.span.clone()].to_owned();
        let remainder = replace(&document.raw, entry.span.clone(), "");
        let remaining = Document::parse(&remainder);
        let target = remaining.section(section)?;
        Ok(Edit {
            content: insert(&remainder, target.span.end, &block, self.newline()),
            entry_id: identified.entry_id,
        })
    }

    pub fn close(&self, selector: &Selector, outcome: &str, date: &str) -> io::Result<Edit> {
        if outcome.trim().is_empty() || outcome.contains(['\r', '\n', '\0']) {
            return Err(invalid("invalid_outcome"));
        }
        let entry = self.entry(selector)?;
        let eol = self.newline();
        let existing = entry.body.trim_end_matches(['\r', '\n']);
        let body = if existing.trim().is_empty() {
            format!("Outcome ({date}): {}{eol}{eol}", outcome.trim())
        } else {
            format!(
                "{existing}{eol}{eol}Outcome ({date}): {}{eol}{eol}",
                outcome.trim()
            )
        };
        let edited = self.edit(selector, &entry.title, &body)?;
        let document = Document::parse(&edited.content);
        let entry = document.entry(&Selector::Id(edited.entry_id.clone()))?;
        let block = document.raw[entry.span.clone()].to_owned();
        let mut remainder = replace(&document.raw, entry.span.clone(), "");
        if !document.sections.iter().any(|section| section.closed) {
            remainder = insert(
                &remainder,
                remainder.len(),
                &format!("## Closed{eol}{eol}"),
                eol,
            );
        }
        let remaining = Document::parse(&remainder);
        let target = remaining.section("Closed")?;
        Ok(Edit {
            content: insert(
                &remainder,
                target
                    .entries
                    .first()
                    .map_or(target.span.end, |entry| entry.span.start),
                &block,
                eol,
            ),
            entry_id: edited.entry_id,
        })
    }
}

fn block(title: &str, body: &str, id: &str, eol: &str) -> io::Result<String> {
    if title.trim().is_empty() || title.contains(['\r', '\n', '\0']) || title.contains("<!-- id:") {
        return Err(invalid("invalid_title"));
    }
    let mut body = body
        .replace("\r\n", "\n")
        .replace('\r', "\n")
        .replace('\n', eol);
    if !body.is_empty() && !body.ends_with('\n') {
        body.push_str(eol);
    }
    let block = format!("### {} <!-- id: {id} -->{eol}{body}", title.trim());
    let document = Document::parse(&block);
    document
        .writable()
        .map_err(|_| invalid("structural_body"))?;
    if document.sections.len() != 1 || document.sections[0].entries.len() != 1 {
        return Err(invalid("structural_body"));
    }
    Ok(block)
}

fn insert(text: &str, offset: usize, block: &str, eol: &str) -> String {
    let prefix = padding(&text[..offset], eol);
    let suffix = if offset < text.len() {
        padding(block, eol)
    } else {
        String::new()
    };
    replace(text, offset..offset, &format!("{prefix}{block}{suffix}"))
}

fn padding(text: &str, eol: &str) -> String {
    if text.is_empty() || text.ends_with(&format!("{eol}{eol}")) {
        String::new()
    } else if text.ends_with('\n') {
        eol.into()
    } else {
        format!("{eol}{eol}")
    }
}
