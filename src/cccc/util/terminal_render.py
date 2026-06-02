from __future__ import annotations

from typing import Optional


_HR_CHARS = set("─━-=═")


def _is_horizontal_rule(line: str) -> bool:
    t = (line or "").strip()
    if len(t) < 20:
        return False
    # Treat long decorative rulers as equivalent even if width varies.
    if all((ch in _HR_CHARS) for ch in t):
        return True
    # Mixed rulers (e.g. some whitespace) should already be stripped out.
    return False


def _normalize_for_compaction(line: str) -> str:
    s = (line or "").rstrip()
    if not s:
        return ""
    if _is_horizontal_rule(s):
        return "<HR>"
    return s


def _parse_csi_params(param_str: str) -> list[int]:
    parts = (param_str or "").split(";") if param_str is not None else []
    out: list[int] = []
    for p in parts:
        p = p.strip()
        if not p:
            continue
        try:
            out.append(int(p))
        except Exception:
            continue
    return out


def _ensure_row(buf: list[list[str]], row: int) -> None:
    while len(buf) <= row:
        buf.append([])


def _ensure_col(line: list[str], col: int) -> None:
    if col < 0:
        return
    if len(line) <= col:
        line.extend([" "] * (col + 1 - len(line)))


def _set_char(buf: list[list[str]], row: int, col: int, ch: str) -> None:
    if row < 0 or col < 0:
        return
    _ensure_row(buf, row)
    line = buf[row]
    _ensure_col(line, col)
    line[col] = ch


def _erase_in_line(buf: list[list[str]], row: int, col: int, mode: int) -> None:
    if row < 0:
        return
    _ensure_row(buf, row)
    line = buf[row]
    if mode == 2:
        # Clear entire line.
        buf[row] = []
        return
    if mode == 1:
        # Clear from start to cursor.
        if col <= 0:
            return
        _ensure_col(line, col - 1)
        for i in range(0, col):
            line[i] = " "
        return
    # mode 0: clear from cursor to end.
    if col < 0:
        col = 0
    if col >= len(line):
        return
    for i in range(col, len(line)):
        line[i] = " "


def _erase_in_display(buf: list[list[str]], row: int, col: int, mode: int) -> None:
    if mode == 2:
        # Clear whole screen.
        buf.clear()
        buf.append([])
        return
    if mode == 1:
        # Clear from start to cursor.
        for r in range(0, max(0, row)):
            if r < len(buf):
                buf[r] = []
        _erase_in_line(buf, row, col, 1)
        return
    # mode 0: clear from cursor to end.
    _erase_in_line(buf, row, col, 0)
    for r in range(row + 1, len(buf)):
        buf[r] = []


def _clone_screen(buf: list[list[str]]) -> list[list[str]]:
    return [list(line) for line in (buf or [[]])]


def _compact_consecutive_duplicate_lines(lines: list[str]) -> list[str]:
    if not lines:
        return []
    out: list[str] = []
    last: Optional[str] = None
    for line in lines:
        cur = _normalize_for_compaction(line)
        if last is not None and cur == last:
            continue
        out.append(line)
        last = cur
    return out


def _compact_consecutive_duplicate_blocks(
    lines: list[str],
    *,
    min_block_lines: int = 3,
    max_block_lines: int = 60,
) -> list[str]:
    """Remove consecutive repeated blocks of lines (common in TUI re-renders).

    This keeps the first occurrence and drops immediately repeated copies.
    """
    n = len(lines)
    if n <= 1:
        return lines

    min_k = max(1, int(min_block_lines))
    max_k_cap = max(1, int(max_block_lines))

    norm = [_normalize_for_compaction(x) for x in lines]

    out: list[str] = []
    i = 0
    while i < n:
        max_k = min(max_k_cap, (n - i) // 2)
        k_found = 0
        # Prefer larger blocks so we collapse whole frames instead of tiny patterns.
        for k in range(max_k, min_k - 1, -1):
            if norm[i : i + k] == norm[i + k : i + 2 * k]:
                k_found = k
                break
        if not k_found:
            out.append(lines[i])
            i += 1
            continue

        # Keep one block, skip subsequent identical blocks.
        out.extend(lines[i : i + k_found])
        i += k_found
        while i + k_found <= n and norm[i : i + k_found] == norm[i - k_found : i]:
            i += k_found

    return out


def render_transcript(text: str, *, compact: bool = True) -> str:
    """Render a best-effort, readable transcript from terminal output.

    Goals:
    - Render terminal output into a stable, readable text view (best-effort)
    - Handle common cursor movement + erase sequences so TUIs don't duplicate frames
    - Optionally compact consecutive duplicated frames (common for TUIs)
    - Keep output readable for debugging and incident review

    This is not a full terminal emulator; it is a pragmatic transcript renderer.
    """
    s = (text or "")
    if not s:
        return ""

    # Normalize common CRLF line endings.
    s = s.replace("\r\n", "\n")

    # Screen buffer.
    buf: list[list[str]] = [[]]
    row = 0
    col = 0
    saved_row = 0
    saved_col = 0
    main_buf: Optional[list[list[str]]] = None
    main_row = 0
    main_col = 0
    in_alt_screen = False

    i = 0
    n = len(s)
    while i < n:
        ch = s[i]

        # ESC sequences.
        if ch == "\x1b":
            if i + 1 >= n:
                break
            nxt = s[i + 1]

            # OSC: ESC ] ... BEL  OR  ESC ] ... ESC \
            if nxt == "]":
                j = i + 2
                while j < n:
                    if s[j] == "\x07":
                        j += 1
                        break
                    if s[j] == "\x1b" and j + 1 < n and s[j + 1] == "\\":
                        j += 2
                        break
                    j += 1
                i = j
                continue

            # CSI: ESC [ ... <final>
            if nxt == "[":
                j = i + 2
                private = False
                if j < n and s[j] == "?":
                    private = True
                    j += 1
                param_start = j
                while j < n and not ("@" <= s[j] <= "~"):
                    j += 1
                if j >= n:
                    break
                final = s[j]
                params = _parse_csi_params(s[param_start:j])

                # Alternate screen buffers are transient TUI state. Preserve the main screen so
                # snapshots after vim/less/top exits do not include stale full-screen content.
                if private:
                    if final in ("h", "l") and 1049 in params:
                        if final == "h" and not in_alt_screen:
                            main_buf = _clone_screen(buf)
                            main_row = row
                            main_col = col
                            buf = [[]]
                            row = 0
                            col = 0
                            saved_row = 0
                            saved_col = 0
                            in_alt_screen = True
                        elif final == "l" and in_alt_screen:
                            buf = _clone_screen(main_buf or [[]])
                            row = main_row
                            col = main_col
                            saved_row = row
                            saved_col = col
                            in_alt_screen = False
                    i = j + 1
                    continue

                # SGR: styles/colors
                if final == "m":
                    i = j + 1
                    continue

                # Cursor position
                if final in ("H", "f"):
                    r = params[0] if len(params) >= 1 else 1
                    c = params[1] if len(params) >= 2 else 1
                    row = max(0, int(r) - 1)
                    col = max(0, int(c) - 1)
                    _ensure_row(buf, row)
                    i = j + 1
                    continue

                # Cursor movements
                if final == "A":  # up
                    k = params[0] if params else 1
                    row = max(0, row - int(k))
                    i = j + 1
                    continue
                if final == "B":  # down
                    k = params[0] if params else 1
                    row = row + int(k)
                    _ensure_row(buf, row)
                    i = j + 1
                    continue
                if final == "C":  # forward
                    k = params[0] if params else 1
                    col = col + int(k)
                    i = j + 1
                    continue
                if final == "D":  # back
                    k = params[0] if params else 1
                    col = max(0, col - int(k))
                    i = j + 1
                    continue
                if final == "G":  # CHA (horizontal absolute)
                    c = params[0] if params else 1
                    col = max(0, int(c) - 1)
                    i = j + 1
                    continue
                if final == "d":  # VPA (vertical absolute)
                    r = params[0] if params else 1
                    row = max(0, int(r) - 1)
                    _ensure_row(buf, row)
                    i = j + 1
                    continue

                # Erase in display / line
                if final == "J":
                    mode = params[0] if params else 0
                    _erase_in_display(buf, row, col, int(mode))
                    row = max(0, min(row, len(buf) - 1))
                    col = max(0, col)
                    i = j + 1
                    continue
                if final == "K":
                    mode = params[0] if params else 0
                    _erase_in_line(buf, row, col, int(mode))
                    i = j + 1
                    continue

                # Save/restore cursor (best-effort)
                if final == "s":
                    saved_row, saved_col = row, col
                    i = j + 1
                    continue
                if final == "u":
                    row, col = saved_row, saved_col
                    _ensure_row(buf, row)
                    i = j + 1
                    continue

                # Unknown CSI: ignore.
                i = j + 1
                continue

            # Other ESC: ignore.
            i += 2
            continue

        # Controls
        if ch == "\n":
            row += 1
            col = 0
            _ensure_row(buf, row)
            i += 1
            continue
        if ch == "\r":
            col = 0
            i += 1
            continue
        if ch == "\b":
            col = max(0, col - 1)
            i += 1
            continue
        if ch == "\x00":
            i += 1
            continue

        # Printable
        _set_char(buf, row, col, ch)
        col += 1
        i += 1

    out_lines = ["".join(line).rstrip() for line in buf]
    # Trim trailing empty lines.
    while out_lines and not out_lines[-1].strip():
        out_lines.pop()

    if compact:
        out_lines = _compact_consecutive_duplicate_lines(out_lines)
        out_lines = _compact_consecutive_duplicate_blocks(out_lines)

    return "\n".join(out_lines).rstrip()
