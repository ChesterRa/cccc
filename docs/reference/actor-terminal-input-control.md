# Actor Terminal Input Control

Date: `2026-03-18`

Status: `reference`

## Purpose

This document explains how to send **raw terminal input** to a running PTY actor in CCCC.

It is intended for:

- Web terminal automation
- Browser / Chrome MCP driven tests
- Future terminal-driving bots
- Debugging cases where an actor is blocked on a TUI prompt such as:
  - `Press enter to continue`
  - trust-directory prompts
  - numbered selection prompts

This document also clarifies the difference between:

- **raw terminal keystrokes**
- **normal chat message delivery**

Those two paths are different and should not be mixed.

## Terminology Alignment

This document follows the local glossary:

- `actor` is the live participant whose PTY you are driving
- raw terminal input is not the same surface as chat delivery
- terminal tail text is useful runtime evidence, but not guaranteed live truth
- `status` should be read as an evidence-bound observation, not as a universal
  proof of every deeper runtime property
- `host_surface` means the CCCC-owned readable surface around PTY transport and
  delivery behavior, not downstream interpretation

## Current Primary Interface

For raw keystrokes, use the actor terminal WebSocket:

`/groups/{group_id}/actors/{actor_id}/term`

Relevant code path:

- `src/cccc/ports/web/routes/actors.py`
- `src/cccc/runners/pty.py`

## How The Terminal WebSocket Works

The WebSocket route accepts a connection, attaches to the actor's PTY stream, then forwards:

- PTY output back to the client as binary frames
- terminal input from the client into the PTY stdin

The input message shape is:

```json
{"t":"i","d":"..."}
```

Where:

- `t = "i"` means terminal input
- `d` is the raw text / control sequence to write into the PTY

The backend behavior is effectively:

1. receive WebSocket text frame
2. parse JSON
3. if `t == "i"`, encode `d` as UTF-8
4. write bytes directly into the attached PTY

This is the only current Web-facing interface that supports sending a **pure Enter key** without any accompanying text.

## Raw Input Examples

### Press Enter

```json
{"t":"i","d":"\r"}
```

Meaning:

- send a carriage return into the PTY
- in most CLI / TUI contexts this behaves like pressing Enter

### Type a command and press Enter

```json
{"t":"i","d":"/status\r"}
```

Meaning:

- type `/status`
- then press Enter

This is the correct shape when you want the terminal to execute a command immediately.

### Move cursor up, then press Enter

```json
{"t":"i","d":"\u001b[A\r"}
```

Meaning:

- `\u001b[A` = Up Arrow
- `\r` = Enter

This is useful for selection prompts where the highlighted option may not already be on the desired line.

### Interrupt with Ctrl+C

```json
{"t":"i","d":"\u0003"}
```

Meaning:

- send ETX / Ctrl+C to the PTY

### Send Escape

```json
{"t":"i","d":"\u001b"}
```

Meaning:

- send Escape

### Move down, then confirm

```json
{"t":"i","d":"\u001b[B\r"}
```

Meaning:

- `\u001b[B` = Down Arrow
- `\r` = Enter

## Common Control Sequences

Useful terminal control inputs for automation:

- `\r` = Enter / carriage return
- `\n` = line feed
- `\u0003` = Ctrl+C
- `\u0004` = Ctrl+D
- `\u001b` = Escape
- `\u001b[A` = Up Arrow
- `\u001b[B` = Down Arrow
- `\u001b[C` = Right Arrow
- `\u001b[D` = Left Arrow

## Important Distinction: Raw Input vs Chat Delivery

CCCC also has normal messaging APIs such as:

- `POST /api/v1/groups/{group_id}/send`
- `POST /api/v1/groups/{group_id}/reply`

Those routes are **not** raw-keyboard APIs.

They go through the daemon message delivery pipeline and ultimately call PTY text submission logic.

That path behaves like:

1. queue a `chat.message`
2. deliver text payload into the PTY
3. automatically append a submit key based on actor `submit` mode

Default submit mode is:

- `b"\r"` for `enter`

This means normal message delivery is best understood as:

- **send text to the actor**
- then **auto-submit**

It is not equivalent to direct keystroke control.

## Why `/send` Cannot Be Used For Pure Enter

The PTY text submission logic rejects empty text before it tries to submit:

- `raw = (text or "").rstrip("\\n")`
- if `raw` is empty, the call returns `False`

So these are different:

Correct for pure Enter:

```json
{"t":"i","d":"\r"}
```

Not valid for pure Enter:

```json
POST /api/v1/groups/{group_id}/send
{
  "text": "",
  "by": "user",
  "to": ["actor-id"]
}
```

The messaging route is suitable for:

- sending chat instructions
- sending `/status` as terminal text plus auto-submit
- normal actor conversation

It is not suitable for:

- arrow keys
- bare Enter
- Escape
- Ctrl+C
- other raw TUI navigation keys

## How To Think About `\r` In Selection Prompts

`\r` does **not** mean “select option 1”.

`\r` means:

- confirm the **currently highlighted** option

So if a prompt visually looks like:

```text
1. Yes, continue
› 2. No, quit

Press enter to continue
```

Then a raw:

```json
{"t":"i","d":"\r"}
```

is more likely to confirm `2. No, quit`, because the highlight marker is on that line.

To force-select `1. Yes, continue`, a safer automation input is:

```json
{"t":"i","d":"\u001b[A\r"}
```

This means:

- move selection up once
- then press Enter

## Important Caution About Terminal Tail Snippets

System notifications often include terminal tail excerpts such as:

- `Press enter to continue`
- the last 20 lines of terminal output

These excerpts are useful for diagnosis, but they are **not guaranteed** to be the exact live TUI state at the moment you send input.

In particular:

- the visible tail may lag the live cursor state
- the currently highlighted option may have changed
- the transcript excerpt may omit control-sequence effects

So:

- use notification text as a hint
- do not treat it as perfect ground truth for current TUI selection state

## Recommended Automation Strategy

For terminal-driving automation, prefer this order:

1. Connect to `/groups/{group_id}/actors/{actor_id}/term`
2. Observe the live PTY output
3. Send raw input frames with `{"t":"i","d":"..."}`
4. Use arrow-key sequences before `\r` when selection matters
5. Reserve `/send` and `/reply` for normal textual interaction, not TUI control

## Practical Examples

### Example: trigger `/status`

```json
{"t":"i","d":"/status\r"}
```

### Example: trust-directory prompt, choose Yes

```json
{"t":"i","d":"\u001b[A\r"}
```

### Example: trust-directory prompt, accept current default

```json
{"t":"i","d":"\r"}
```

### Example: stop a stuck command

```json
{"t":"i","d":"\u0003"}
```

## Current Limitations

- There is currently no dedicated HTTP endpoint for “raw PTY input bytes”.
- The supported Web-facing raw-input surface is the actor terminal WebSocket.
- Normal messaging APIs intentionally operate at the chat-message layer, not the keystroke layer.

## Relevant Source References

- `src/cccc/ports/web/routes/actors.py`
- `src/cccc/runners/pty.py`
- `src/cccc/daemon/messaging/delivery.py`
- `web/src/components/AgentTab.tsx`

## Related Glossary

- [actor](/reference/glossary/actor)
- [status](/reference/glossary/status)
- [host_surface](/reference/glossary/host_surface)

## Change Log

- `2026-03-24`: Added glossary alignment so terminal-input docs keep raw PTY control, chat delivery, and evidence-bound status semantics clearly separated.
