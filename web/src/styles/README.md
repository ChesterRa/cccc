# LCD style hooks

LCD is the Web UI's only look. Components still carry the classes of the former classic
look (comments in this folder call them "classic") and gain `data-*` hooks; the LCD rules
in this folder read the hooks. When you edit a component, you may drop classic palette
classes that its hooks already override.

## Scope

- `index.html` sets `data-ui-style="lcd"` on `<html>` and `data-lcd-scope` on `<body>`, so
  the whole page and every layer portalled into the body are in scope from the first
  paint. Rules start with `:root[data-ui-style="lcd"]`, and rules that restyle shared
  components add `[data-lcd-scope]`; follow that form in new rules so their specificity
  matches. The prefix no longer limits where a rule applies.
- Every colour should resolve to an `--lcd-*` token. A Tailwind palette class
  (`text-rose-600`, `bg-black/5`) needs a hook below; do not add an LCD rule keyed on the
  palette class.

## Choose a hook by meaning, not by the classic colour

| Hook                    | Values                                                 | Use for                                                                                                                                                                                                                          |
| ----------------------- | ------------------------------------------------------ | -------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `data-status-tone`      | `error`, `warning`, `info`, `ok`, `neutral`, `muted`   | Notices, status text, pills and buttons that report a state. `neutral` is for states that are neither good nor bad news ("optional", "hidden"); it looks like `ok`, as success has no colour of its own. `muted` only dims text. |
| `data-status-dot`       | `active`, `attention`, `failed`, `inactive`, `neutral` | Small status marks. `attention` is something in progress or waiting; `neutral` is worth a glance but neither working nor waiting on the user.                                                                                    |
| `data-pending-state`    | presence                                               | A count or tag of things waiting on the user: the one filled "waiting on you" tag. Plain unread counts are not this.                                                                                                             |
| `destructive-key` class | from `Button variant="destructive"`                    | Actions that delete or discard.                                                                                                                                                                                                  |

## Controls

- `data-lcd-key`: a text action, or a secondary key beside a primary one. It is framed at
  rest and reversed under the pointer. Not for list rows or primary buttons.
- `data-lcd-primary`: a primary key built by hand instead of with `Button` (whose primary
  variant already follows the LCD tokens). It is filled in ink.
- `data-lcd-field`: a text field or select built by hand instead of with `Input` (whose
  `glass-input` already takes the strong rule in dialogs). It is framed in the strong rule,
  and in ink while focused.
- Recipient chips are independent `aria-pressed` toggles, so they do not share edges.
  Selected chips in `data-composer-recipient-row` and `data-relay-recipient-row` use the
  same reversed treatment in `lcdComposer.css`.
- Switches: a container of `aria-pressed` buttons (or `role="radio"` items) whose keys
  share edges, the current one reversed. Add a new switch's selector to the shared lists
  in `lcdModal.css` (`data-settings-scope-switch`, `data-segmented-choice`,
  `data-search-kind`, `data-voice-pane-switch`, `data-workspace-mode-switch`,
  `data-connect-mode-switch`, `data-browser-view-switch`, `data-voice-panel-views`,
  `data-voice-document-views`, `data-presentation-snapshot-tabs`,
  `data-presentation-preview-mode`, `data-presentation-slot-switch`, and
  `data-web-model-delivery`, whose keys are the visible spans of radio labels); do not
  write another rule set. The hook goes on the element whose children are the keys.
- Menus: `data-group-menu` or `data-lcd-menu` on the menu surface, portalled or not.
  Its items (`menuitem*` and `option` elements, or plain buttons) are ink throughout and
  reversed under the pointer, keyboard focus or a list's `aria-selected`; a current item
  keeps its check mark rather than a fill. Inside an item, `data-lcd-hint` marks a dim
  secondary line and `data-lcd-tag` a framed kind label; `data-option-mark` is a list's
  own selection bar, hidden here. A destructive item takes `data-tone="danger"` or
  `destructive-key`.

## Control sizes

One control mixed three scales: frames in `rem`, avatars, icons and labels in fixed
pixels. At a larger text size the frames grew and the rest did not, so avatars looked a
size smaller than the keys beside them and labels looked thin.

- Size controls in `rem`, so the frame, drawing and words follow the text size together.
  Rules and borders stay at 1px.
- A standard key is 2rem square (2rem high with a label). Touch screens keep the global
  44px minimum target from `index.css` at every text size; a key sized in `rem` may grow
  past it, and the drawing grows with the text.
- Icons in a standard key are 1rem; in a 44px touch key, 1.5rem. Pixelarticons are
  sharpest at 24px, but a single size across the screen matters more than a crisp exception.
- Words on a key are 0.75rem at weight 500. Do not set 600 or 700: the pixel font has
  no true bold, and a synthetic one blurs.
- A member avatar is a 2.5rem key around its 2rem tile. Its state lines are drawn outside
  the tile: the busy ring of pixel dots, the double line for needs attention, and the
  open line under the key (`lcdMotion.css`).

## Icons

Every icon is a pixel glyph from `components/LcdGlyph.tsx`: Pixelarticons 2.4.2, plus the
few original glyphs that file names. Runtime logos, Chet's head (`features/chet/ChetHead.tsx`)
and `LcdSpinner` are drawn on their own. Use an export from `components/Icons.tsx`, or
`LcdIcon` with a glyph around a Lucide component; never render a Lucide icon directly. Don't
give an icon children of its own: it then keeps its Lucide drawing, without a glyph.

- When code picks an icon at run time (a condition or a table), pick its glyph by the same
  condition, so that states such as shown/hidden or file/link stay distinct.
- Take a missing glyph from the Pixelarticons package before drawing one. Check a new glyph
  at the sizes where it appears, often 14px, not only enlarged: on the 2px grid a short
  arrowhead reads as a plus.
- `Icons.lcd.test.tsx` checks two common slips: an `Icons.tsx` export without a glyph, and a
  component that renders a Lucide icon it imports by name directly as JSX. It does not see an
  icon picked through a variable or a table, an icon given children, or other inline SVG;
  check those where they are written and on the page.

## Motion

Motion moves in whole steps (`lcdMotion.css`): loops for work that runs now, which stop
with it; one-shot feedback; and the composer's typing hint. Each loop and blink has a still
form for `data-lcd-motion="off"`, which the root carries when the user turns motion off in
Appearance or the system asks for reduced motion; write the still form as its own rule
rather than relying on the `index.css` reduce rule.

- Busy member: `.lcd-member-ring` with `data-ring="moving|still"` (RuntimeDock sets
  `--lcd-ring-delay` so rings that start at different times move in step).
- Work the page sees return to idle: `data-flourish` lights the ring for 250 ms.
- Waiting on the person: `data-attention-flash` blinks the attention lines three times.
- A wait of unknown length: `LcdSpinner`. The caller changes its label and disabled state
  at once; the cells appear after 250 ms, and a button passes its own icon as `children`
  to keep it until then.
- A status dot of 8px or less whose work runs now: `data-status-motion="working"`.
- Live text: `data-lcd-live-cursor="solid|waiting|quiet"` (HeadlessLiveTrace).
- Reconnecting: `.lcd-reconnect-dots`.
- The composer's typing hint: `data-composer-caret="typing|hold"`
  (ComposerTypingPlaceholder); with motion off the hint does not type.
- Chet's head (`features/chet/ChetHead.tsx`): its frames are stacked in one SVG and swap in
  whole steps by `data-chet-head="idle|working"`; the still form shows the face of the
  current state. A still rule must outrank each frame's animation, not just follow it.

## A surface built from many palette classes

When a component paints most of its parts with palette classes (the headless trace, the
setup checklist), mark its root and give the root's descendants a default inside a
`:where()` selector, so every hook above still wins: text inherits the ink, and where
needed backgrounds clear and borders take the rule. Then hook only what carries meaning:
cards, live or done markers, muted labels, tones. See `data-headless-trace` and
`data-setup-checklist` in `lcd.css`.

## Kept as they are

Terminal canvases (xterm and its ANSI colours), runtime logos (marked
`data-runtime-logo`), presentation content, the projected browser's page, and document,
PDF and media previews keep their own colours. A member tile that shows a runtime logo
keeps the logo's white ground.

## Before handing over an adapted surface

- Open it in light and dark, at desktop and phone widths, and confirm no colour falls
  outside the `--lcd-*` tokens, apart from the content listed above. Include the states a
  first look misses: errors, loading, hover, focus and disabled.
- Confirm every visible icon is a pixel glyph (`svg[data-lcd-icon]`), apart from the
  exceptions named under Icons.
