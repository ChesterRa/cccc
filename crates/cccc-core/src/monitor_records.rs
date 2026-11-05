//! Fits a native notice to what Claude Code shows of a Monitor command's output.
//!
//! Claude Code 2.1.317 keeps 500 UTF-16 units of each stdout line and 3,000 of
//! the lines written together, marking either cut with `...(truncated)`. These
//! are observed limits, not a vendor contract, so both budgets leave room.

use serde_json::json;

const RECORD_UNITS: usize = 480;
const NOTICE_UNITS: usize = 2_800;
/// The units `{"message":""}` adds to every record.
const RECORD_ENVELOPE_UNITS: usize = 14;

/// The stdout records that carry `text`, each within the record budget, in order.
#[must_use]
pub fn records(text: &str) -> Vec<String> {
    let mut records = Vec::new();
    let mut rest = text;
    while !rest.is_empty() {
        let (cut, _) = record_end(rest);
        records.push(record(&rest[..cut]));
        rest = &rest[cut..];
    }
    records
}

/// Whether `text` reaches the model whole when its records are written together.
/// The model reads each record's JSON as it stands, so records must end after a
/// line break, a space or a non-ASCII character; a long run of ASCII that offers
/// none of those is left to the exact read.
#[must_use]
pub fn fits_one_notice(text: &str) -> bool {
    let mut units = 0;
    let mut rest = text;
    while !rest.is_empty() {
        let (cut, clean) = record_end(rest);
        if !clean {
            return false;
        }
        units += record(&rest[..cut]).encode_utf16().count() + 1;
        rest = &rest[cut..];
    }
    units <= NOTICE_UNITS
}

fn record(text: &str) -> String {
    json!({"message":text}).to_string()
}

/// Where the next record ends, and whether that is between words. It prefers the
/// last line break in its second half, then the last space or non-ASCII
/// character, which covers scripts written without spaces.
fn record_end(text: &str) -> (usize, bool) {
    let mut units = RECORD_ENVELOPE_UNITS;
    let mut end = 0;
    let mut line_end = 0;
    let mut word_end = 0;
    for (index, character) in text.char_indices() {
        units += json_units(character);
        if units > RECORD_UNITS {
            return if line_end * 2 >= end {
                (line_end, true)
            } else if word_end * 2 >= end {
                (word_end, true)
            } else {
                (end, false)
            };
        }
        end = index + character.len_utf8();
        if character == '\n' {
            line_end = end;
        }
        if character.is_whitespace() || !character.is_ascii() {
            word_end = end;
        }
    }
    (end, true)
}

fn json_units(character: char) -> usize {
    match character {
        '"' | '\\' | '\n' | '\r' | '\t' | '\u{8}' | '\u{c}' => 2,
        control if control < ' ' => 6,
        other => other.len_utf16(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::Value;

    fn joined(records: &[String]) -> String {
        records
            .iter()
            .map(|record| {
                let value: Value = serde_json::from_str(record).expect("JSON record");
                value["message"].as_str().expect("message").to_owned()
            })
            .collect()
    }

    #[test]
    fn every_record_fits_its_budget_and_together_they_rebuild_the_text() {
        let samples = [
            "short".to_owned(),
            "x".repeat(5_000),
            "界".repeat(5_000),
            "😀".repeat(5_000),
            "\"\\\n\t\r\u{8}\u{c}\u{1}".repeat(1_000),
            format!("{}\n{}", "a".repeat(300), "b".repeat(1_000)),
        ];
        for text in samples {
            let records = records(&text);
            for record in &records {
                let units = record.encode_utf16().count();
                assert!(units <= RECORD_UNITS, "{units} units");
            }
            assert_eq!(joined(&records), text);
        }
    }

    #[test]
    fn a_record_ends_at_a_late_line_break_but_not_an_early_one() {
        let late = format!("{}\n{}", "a".repeat(400), "b".repeat(400));
        assert_eq!(
            joined(&records(&late)[..1]),
            format!("{}\n", "a".repeat(400))
        );

        let early = format!("{}\n{}", "a".repeat(10), "b".repeat(1_000));
        assert!(joined(&records(&early)[..1]).ends_with('b'));
    }

    #[test]
    fn a_record_ends_between_words_when_it_can() {
        let prose = "word ".repeat(300);
        for record in &records(&prose)[..2] {
            let value: Value = serde_json::from_str(record).expect("JSON record");
            assert!(value["message"].as_str().expect("message").ends_with(' '));
        }
        assert!(fits_one_notice(&prose));
        assert!(fits_one_notice(&"第一段文字。".repeat(300)));
    }

    #[test]
    fn a_word_too_long_for_a_record_keeps_the_notice_from_claiming_it_whole() {
        let url = format!("see https://example.test/{}", "a".repeat(600));
        assert_eq!(joined(&records(&url)), url);
        assert!(!fits_one_notice(&url));
    }

    #[test]
    fn a_notice_fits_within_its_budget_counted_as_written() {
        assert!(fits_one_notice("short"));
        assert!(fits_one_notice(&"界".repeat(2_600)));
        assert!(!fits_one_notice(&"界".repeat(2_800)));
        assert!(!fits_one_notice(&"😀".repeat(1_400)));
        assert!(!fits_one_notice(&"\"".repeat(1_400)));
    }
}
