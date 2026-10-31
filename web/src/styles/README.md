# LCD style hooks

The LCD look is a preview style. Classic stays the default and must not change when a
surface is adapted, so components keep their classic classes and gain `data-*` hooks;
the LCD rules in this folder read the hooks.

## Scope

- Every LCD rule starts with `:root[data-ui-style="lcd"]`, and rules that restyle shared
  components also require `[data-lcd-scope]`: a surface takes the look only inside an
  element with `data-lcd-scope`.
- Dialogs: `ModalFrame lcdScope`, or `DialogContent lcdScope` for the Radix dialog in
  `ui/dialog.tsx`. Portalled layers built by hand: `BodyPortal lcdScope`.
- Popovers (`ui/popover.tsx`) take the scope of the control that opened them, found
  through Radix's `aria-controls`. A popover anchored to a point rather than a trigger
  must pass `data-lcd-scope` itself (see `WorkspaceEntryMenu`).
- Inside a scope, every colour should resolve to an `--lcd-*` token. A Tailwind palette
  class (`text-rose-600`, `bg-black/5`) needs a hook below; do not add an LCD rule keyed
  on the palette class.

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
- Menus: `data-group-menu` or `data-lcd-menu` on the menu surface, whether it is
  portalled with its own scope or sits inside a scoped surface. Its items (`menuitem*`
  and `option` elements, or plain buttons) are ink throughout and reversed under the
  pointer, keyboard focus or a list's `aria-selected`; a current item keeps its check
  mark rather than a fill. Inside an item, `data-lcd-hint` marks a dim secondary line and
  `data-lcd-tag` a framed kind label; `data-option-mark` is a list's own selection bar,
  hidden here. A destructive item takes `data-tone="danger"` or `destructive-key`.

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
- A member avatar is a key: its tile fills the 2rem frame, and its state lines (busy,
  needs attention, open) are drawn inside the tile.

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
PDF and media previews keep their own colours inside an LCD scope.

## Before handing over an adapted surface

- Open it in LCD light and dark and confirm no colour inside the scope falls outside the
  `--lcd-*` tokens, apart from the content listed above.
- Compare classic before and after; it should be pixel-identical unless the change is a
  shared fix meant for both looks, which then needs its own note.
