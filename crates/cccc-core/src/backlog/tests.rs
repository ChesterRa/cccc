use super::*;

const A: &str = "a123456789bc";
const B: &str = "b123456789cd";

#[test]
fn parsing_keeps_every_byte_and_ignores_fenced_headings() {
    for (text, expected_titles) in [
        (
            "# 路线\r\n\r\n## 以后\r\n说明\r\n### 一个课题 <!-- id: a123456789bc -->\r\n正文\r\n#### Details\r\n```md\r\n## 假分区\r\n### 假课题\r\n```\r\n## Closed\r\n",
            vec!["一个课题"],
        ),
        (
            "# Plan\n### Topic\n~~~\n### not a topic\n~~~\nNo final newline",
            vec!["Topic"],
        ),
        ("# Plan without a section or trailing newline", vec![]),
    ] {
        let doc = Document::parse(text);
        assert_eq!(doc.content().as_bytes(), text.as_bytes());
        assert!(doc.problems.iter().all(|p| p.code == "missing_id"));
        let titles = doc
            .sections
            .iter()
            .flat_map(|section| &section.entries)
            .map(|entry| entry.title.as_str())
            .collect::<Vec<_>>();
        assert_eq!(titles, expected_titles);
        assert!(!doc.sections.iter().any(|section| section.name == "假分区"));
    }
}

#[test]
fn identify_and_edit_preserve_neighbors_and_stable_identity() {
    let text = format!(
        "# Plan\n\n## Open\n\n### One\nBody\n\n### Two <!-- id: {B} -->\nUntouched\n\n## Closed\n"
    );
    let identified = Document::parse(&text)
        .identify(&Selector::Line(5))
        .expect("valid test fixture");
    assert_eq!(
        identified
            .content
            .replace(&format!(" <!-- id: {} -->", identified.entry_id), ""),
        text
    );
    let doc = Document::parse(&identified.content);
    assert_eq!(
        doc.identify(&Selector::Id(identified.entry_id.clone()))
            .expect("valid test fixture")
            .content,
        identified.content
    );
    let edited = doc
        .edit(
            &Selector::Id(identified.entry_id.clone()),
            "Renamed",
            "New body\n\n",
        )
        .expect("valid test fixture");
    assert_eq!(edited.entry_id, identified.entry_id);
    assert!(edited.content.ends_with(&format!(
        "### Two <!-- id: {B} -->\nUntouched\n\n## Closed\n"
    )));
    assert!(edited.content.starts_with("# Plan\n\n## Open\n\n"));
}

#[test]
fn moves_preserve_bodies_and_close_adds_an_explicit_outcome() {
    let text = format!(
        "# Plan\n## Open\n### One <!-- id: {A} -->\nA\n\n### Two <!-- id: {B} -->\nB\n## Later\n"
    );
    let moved = Document::parse(&text)
        .move_entry(&Selector::Id(A.into()), "Later")
        .expect("valid test fixture");
    assert!(
        moved
            .content
            .ends_with(&format!("## Later\n\n### One <!-- id: {A} -->\nA\n\n"))
    );
    let closed = Document::parse(&moved.content)
        .close(&Selector::Id(A.into()), "Shipped", "2026-11-11")
        .expect("valid test fixture");
    let doc = Document::parse(&closed.content);
    let last = doc.sections.last().expect("valid test fixture");
    assert!(last.closed);
    assert_eq!(last.entries[0].id.as_deref(), Some(A));
    assert!(
        last.entries[0]
            .body
            .contains("Outcome (2026-11-11): Shipped")
    );
    assert!(
        closed
            .content
            .contains(&format!("### Two <!-- id: {B} -->\nB\n"))
    );
}

#[test]
fn ambiguous_ids_sections_invalid_selectors_and_structural_body_are_rejected() {
    let doc = Document::parse(&format!(
        "## Open\n### One <!-- id: {A} -->\na\n### Two <!-- id: {A} -->\nb\n## Open\n"
    ));
    assert!(doc.identify(&Selector::Id(A.into())).is_err());
    assert!(doc.add("Open", "Three", "").is_err());
    assert!(
        doc.close(&Selector::Id(A.into()), "done", "2026-11-11")
            .is_err()
    );
    let doc = Document::parse("## Open\n### One\nBody\n");
    assert!(doc.identify(&Selector::Line(3)).is_err());
    assert!(doc.add("Open", "bad", "## Another section\n").is_err());
    assert!(doc.add("Open", "bad", "### Another entry\n").is_err());
    assert_eq!(
        doc.add("Open", "bad", "```\nUnclosed draft")
            .err()
            .expect("invalid body")
            .to_string(),
        "structural_body"
    );
    assert!(
        Document::parse("## Open\n```\n")
            .add("Open", "bad", "")
            .is_err()
    );
    assert!(doc.add("Missing", "bad", "").is_err());
    assert!(doc.add("Open", "valid", "```\n### Example\n```\n").is_ok());
}

#[test]
fn crlf_and_no_final_newline_are_preserved_when_identifying() {
    let text = "## Open\r\n### Topic";
    let identified = Document::parse(text)
        .identify(&Selector::Line(2))
        .expect("valid test fixture");
    assert_eq!(
        identified.content,
        format!("## Open\r\n### Topic <!-- id: {} -->", identified.entry_id)
    );
    let text = format!("## Open\r\n### Topic <!-- id: {A} -->\r\nBody");
    let moved = Document::parse(&text)
        .move_entry(&Selector::Id(A.into()), "Open")
        .expect("valid test fixture");
    assert_eq!(moved.content, text);
    let mixed = "# Plan\n## Open\r\n###   Topic  \t\r\nBody\n";
    let identified = Document::parse(mixed)
        .identify(&Selector::Line(3))
        .expect("identify mixed heading");
    assert_eq!(
        identified
            .content
            .replace(&format!(" <!-- id: {} -->", identified.entry_id), ""),
        mixed
    );
}

#[test]
fn replacement_and_close_keep_readable_separators_and_native_newlines() {
    let text = format!(
        "## Open\r\n\r\n### One <!-- id: {A} -->\r\n\r\nBody\r\n\r\n## Closed\r\n\r\n### Old <!-- id: {B} -->\r\nOld result\r\n"
    );
    let doc = Document::parse(&text);
    let edited = doc
        .edit(&Selector::Id(A.into()), "One", "\nNew body")
        .expect("valid test fixture");
    assert!(edited.content.contains("New body\r\n\r\n## Closed"));
    assert!(!edited.content.replace("\r\n", "").contains('\n'));
    let closed = doc
        .close(&Selector::Id(A.into()), "Done", "2026-11-11")
        .expect("valid test fixture");
    assert!(
        closed
            .content
            .contains(&format!("## Closed\r\n\r\n### One <!-- id: {A} -->"))
    );
    assert!(
        closed
            .content
            .contains("Body\r\n\r\nOutcome (2026-11-11): Done\r\n\r\n### Old")
    );
    assert!(!closed.content.contains("Body\r\n\r\n\r\nOutcome"));
    assert!(doc.add("Closed", "Cannot skip outcome", "").is_err());
}
