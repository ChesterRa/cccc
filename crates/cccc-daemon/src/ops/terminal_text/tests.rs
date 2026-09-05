use super::render;

#[test]
fn renders_csi_and_osc_sequences_instead_of_leaking_them() {
    let raw = "\u{1b}[2J\u{1b}[1;1HWorking\u{1b}[49m\u{1b}]0;⠴ wechat-agent\u{7}\r\nready";
    assert_eq!(render(raw, false), "Working\nready");
}

#[test]
fn cursor_positioning_overwrites_previous_tui_frame() {
    let raw = "\u{1b}[1;1Hold frame\u{1b}[1;1H\u{1b}[2Knew frame";
    assert_eq!(render(raw, true), "new frame");
}

#[test]
fn carriage_return_overwrites_the_current_line() {
    assert_eq!(render("old\rnew", true), "new");
}

#[test]
fn wide_characters_keep_terminal_cursor_columns_aligned() {
    assert_eq!(render("你好\u{1b}[1;5HX", true), "你好X");
}
