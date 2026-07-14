use serde_json::{Value, json};
use std::path::Path;
use std::process::Command;

pub fn git_status(root: &Path) -> Value {
    let output = git(root, &["status", "--short"]);
    let changed_files: Vec<_> = output
        .lines()
        .filter_map(|line| {
            line.get(3..)
                .map(str::trim)
                .filter(|value| !value.is_empty())
        })
        .collect();
    json!({"changed_files":changed_files,"summary":output})
}

pub fn git_diff(root: &Path, path: &str) -> Value {
    let stat = git(root, &["diff", "--stat"]);
    let name_status = git(root, &["diff", "--name-status"]);
    let diff = if path.is_empty() {
        git(root, &["diff"])
    } else {
        git(root, &["diff", "--", path])
    };
    json!({"stat":stat,"name_status":name_status,"diff":diff})
}

pub fn html(title: &str, content: &str) -> String {
    format!(
        "<!doctype html><html><head><meta charset=\"utf-8\"><title>{}</title></head><body><main><h1>{}</h1><pre>{}</pre></main></body></html>",
        escape(title),
        escape(title),
        escape(content)
    )
}

fn git(root: &Path, args: &[&str]) -> String {
    Command::new("git")
        .args(args)
        .current_dir(root)
        .output()
        .ok()
        .filter(|output| output.status.success())
        .map(|output| String::from_utf8_lossy(&output.stdout).into_owned())
        .unwrap_or_default()
}

fn escape(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}
