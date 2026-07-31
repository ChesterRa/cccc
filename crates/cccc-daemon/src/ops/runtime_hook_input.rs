use cccc_core::HomeLayout;
use std::collections::HashMap;
use std::sync::{Mutex, OnceLock};

const PASTE_START: &[u8] = b"\x1b[200~";
const PASTE_END: &[u8] = b"\x1b[201~";
type Key = (String, String);
type InputState = (bool, Vec<u8>);
type InputStates = HashMap<Key, InputState>;

fn states() -> &'static Mutex<InputStates> {
    static STATES: OnceLock<Mutex<InputStates>> = OnceLock::new();
    STATES.get_or_init(|| Mutex::new(HashMap::new()))
}

pub fn observe(home: &HomeLayout, group_id: &str, actor_id: &str, data: &[u8]) {
    if data.is_empty() {
        return;
    }
    let Ok(status) = cccc_runtime::status(group_id, actor_id) else {
        return;
    };
    let runtime = ["codex", "claude"].into_iter().find(|runtime| {
        super::runtime_hook_session::validated(home, runtime, group_id, actor_id, status.pid)
            .is_some()
    });
    let Some(runtime) = runtime else {
        return;
    };
    let outside = outside_bracketed_paste(group_id, actor_id, data);
    if outside.contains(&0x03) || outside == b"\x1b" {
        let _ = cccc_core::codex_hook_state::record_interrupt(home, runtime, group_id, actor_id);
    } else if runtime == "claude" && (outside.ends_with(b"\r") || outside.ends_with(b"\n")) {
        let _ =
            cccc_core::codex_hook_state::record_terminal_input(home, "claude", group_id, actor_id);
    }
}

pub fn reset(group_id: &str, actor_id: &str) {
    states()
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .remove(&(group_id.to_owned(), actor_id.to_owned()));
}

fn outside_bracketed_paste(group_id: &str, actor_id: &str, data: &[u8]) -> Vec<u8> {
    let key = (group_id.to_owned(), actor_id.to_owned());
    let mut states = states()
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let (mut inside, mut tail) = states.remove(&key).unwrap_or_default();
    tail.extend_from_slice(data);
    let mut source = tail.as_slice();
    let mut output = Vec::new();
    let next_tail;
    loop {
        let marker = if inside { PASTE_END } else { PASTE_START };
        if let Some(index) = find(source, marker) {
            if !inside {
                output.extend_from_slice(&source[..index]);
            }
            source = &source[index + marker.len()..];
            inside = !inside;
            continue;
        }
        let keep = marker_prefix_suffix(source, marker);
        if !inside {
            output.extend_from_slice(&source[..source.len().saturating_sub(keep)]);
        }
        next_tail = source[source.len().saturating_sub(keep)..].to_vec();
        break;
    }
    states.insert(key, (inside, next_tail));
    output
}

fn find(source: &[u8], marker: &[u8]) -> Option<usize> {
    source
        .windows(marker.len())
        .position(|window| window == marker)
}

fn marker_prefix_suffix(source: &[u8], marker: &[u8]) -> usize {
    (1..marker.len().min(source.len() + 1))
        .rev()
        .find(|size| source.ends_with(&marker[..*size]))
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::outside_bracketed_paste;

    #[test]
    fn ignores_newlines_inside_split_bracketed_paste() {
        assert!(outside_bracketed_paste("g_input", "peer", b"\x1b[200~hello\n").is_empty());
        assert_eq!(
            outside_bracketed_paste("g_input", "peer", b"world\x1b[201~\r"),
            b"\r"
        );
    }
}
