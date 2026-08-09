use cccc_contracts::Event;
use cccc_core::{GroupDoc, actors};
use serde_json::{Map, Value};

const QUOTE_PREVIEW_CHARS: usize = 100;

pub fn add_sender_snapshot(group: &GroupDoc, by: &str, data: &mut Map<String, Value>) {
    let Some(actor) = actors::find(group, by) else {
        return;
    };
    insert_snapshot(data, "sender_title", actor.title.clone());
    insert_snapshot(
        data,
        "sender_runtime",
        serde_json::to_value(actor.runtime)
            .ok()
            .and_then(|value| value.as_str().map(str::to_owned))
            .unwrap_or_default(),
    );
    insert_snapshot(data, "sender_avatar_path", actor.avatar_asset_path.clone());
}

pub fn add_sender_title_snapshot(group: &GroupDoc, by: &str, data: &mut Map<String, Value>) {
    let Some(actor) = actors::find(group, by) else {
        return;
    };
    insert_snapshot(data, "sender_title", actor.title.clone());
}

pub fn add_reply_snapshot(target: &Event, data: &mut Map<String, Value>) {
    let Some(text) = target.data.get("text").and_then(Value::as_str) else {
        return;
    };
    let text = text.trim();
    if text.is_empty() {
        return;
    }
    let mut chars = text.chars();
    let preview = chars.by_ref().take(QUOTE_PREVIEW_CHARS).collect::<String>();
    let preview = if chars.next().is_some() {
        format!("{preview}...")
    } else {
        preview
    };
    data.insert("quote_text".into(), Value::String(preview));
}

fn insert_snapshot(data: &mut Map<String, Value>, key: &str, value: String) {
    if value.trim().is_empty() {
        data.remove(key);
    } else {
        data.insert(key.into(), Value::String(value));
    }
}
