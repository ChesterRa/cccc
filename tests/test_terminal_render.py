"""Unit tests for the PTY transcript renderer (`render_transcript`).

`render_transcript` powers the text `<pre>` views (terminal tail, debug
snapshot, automation engine). The live xterm path no longer uses it — that
replays raw bytes and lets xterm emulate — so these direct unit tests are the
authoritative coverage for the renderer's screen approximation.
"""

from cccc.util.terminal_render import render_transcript


def _csi_to_col(col_1based: int) -> str:
    # CHA (Cursor Horizontal Absolute), 1-based column.
    return f"\x1b[{col_1based}G"


class TestWideCharRendering:
    """CJK / fullwidth glyphs occupy two cells; the renderer must not leave a
    padding space in the skipped trailing cell."""

    def test_back_to_back_cjk_has_no_gap(self):
        assert render_transcript("远端构建", compact=False) == "远端构建"

    def test_absolute_positioned_cjk_has_no_gap(self):
        # Mimic a TUI placing each wide glyph by absolute column:
        # 远 at col 1, 端 at col 3, 构 at col 5 (1-based; width 2 each).
        stream = "远" + _csi_to_col(3) + "端" + _csi_to_col(5) + "构"
        assert render_transcript(stream, compact=False) == "远端构"

    def test_mixed_ascii_and_cjk(self):
        text = "使用 tag 发布"
        assert render_transcript(text, compact=False) == text

    def test_pure_ascii_unaffected(self):
        assert render_transcript("Working 1m 20s", compact=False) == "Working 1m 20s"

    def test_cjk_overwrite_replaces_both_cells(self):
        # Writing single-width chars over a wide glyph must not resurrect the
        # filler as a gap.
        stream = "远" + "\r" + "ab"
        assert render_transcript(stream, compact=False) == "ab"


class TestScreenControl:
    """Cursor/erase handling so TUIs don't duplicate frames in the snapshot."""

    def test_clear_screen_then_home_keeps_only_current_frame(self):
        # "first line" is wiped by ED(2) + cursor-home before "current".
        stream = "first line\n\x1b[2J\x1b[Hcurrent"
        text = render_transcript(stream, compact=False)
        assert text == "current"
        assert "\x1b" not in text

    def test_alternate_screen_content_discarded_on_exit(self):
        # Entering (?1049h) then leaving (?1049l) the alt screen must restore the
        # main screen and drop the transient TUI buffer (e.g. vim).
        stream = "shell prompt\n\x1b[?1049h\x1b[Hvim buffer\x1b[?1049l\r\nshell prompt"
        text = render_transcript(stream, compact=False)
        assert "shell prompt" in text
        assert "vim buffer" not in text
