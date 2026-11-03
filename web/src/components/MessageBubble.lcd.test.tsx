// @vitest-environment happy-dom
import { act } from "react";
import { readFileSync } from "node:fs";
import { fileURLToPath, URL as NodeURL } from "node:url";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, expect, it, vi } from "vite-plus/test";
import { MessageBubble } from "./MessageBubble";
import type { LedgerEvent } from "../types";

vi.mock("react-i18next", () => ({ useTranslation: () => ({ t: (key: string) => key }) }));

Object.assign(globalThis, { IS_REACT_ACT_ENVIRONMENT: true });

const event: LedgerEvent = {
  id: "e1",
  kind: "chat.message",
  by: "claude",
  ts: "2026-10-30T16:00:00Z",
  data: { text: "Hello", to: ["user"], message_mode: "send" },
};

let host: HTMLDivElement;
let root: Root;

beforeEach(() => {
  host = document.createElement("div");
  document.body.appendChild(host);
  root = createRoot(host);
});

afterEach(() => {
  act(() => root.unmount());
  host.remove();
});

async function render(
  shown: LedgerEvent = event,
  readOnly = false,
  isHighlighted = false,
  sameSender = false,
) {
  await act(async () =>
    root.render(
      <MessageBubble
        event={shown}
        readOnly={readOnly}
        isHighlighted={isHighlighted}
        sameSender={sameSender}
        actorById={new Map()}
        actors={[]}
        displayNameMap={new Map()}
        agentState={null}
        taskById={new Map()}
        isDark={false}
        groupId="g1"
        groupLabelById={{}}
        onReply={() => undefined}
        onShowRecipients={() => undefined}
        onCopyLink={() => undefined}
      />,
    ),
  );
}

it("puts the pointer's actions under the message and touch's ⋮ at the end of its head", async () => {
  await render();
  const bar = host.querySelectorAll("[data-message-actions-inline]");
  expect(bar).toHaveLength(1);
  expect(bar[0].closest(".lcd-row-body")).not.toBeNull();
  expect(bar[0].closest(".lcd-row-head")).toBeNull();
  const menu = host.querySelectorAll(".message-actions-menu");
  expect(menu).toHaveLength(1);
  expect(menu[0].parentElement?.classList.contains("lcd-row-head")).toBe(true);
  expect(menu[0].nextElementSibling).toBeNull();
});

const lcdCss = () =>
  readFileSync(fileURLToPath(new NodeURL("../styles/lcd.css", import.meta.url)), "utf8");
const chatMessageCss = () =>
  readFileSync(fileURLToPath(new NodeURL("../styles/chatMessage.css", import.meta.url)), "utf8");

it("lays the pointer's actions over the space under a message, reserving none", () => {
  const pointer = chatMessageCss().match(
    /@media \(hover: hover\) and \(pointer: fine\) \{([\s\S]*?)\n\}/,
  )?.[1];
  expect(pointer).toMatch(/\.message-actions-menu \{\s*display: none;/);
  expect(pointer).toMatch(
    /\.message-actions-inline \{\s*position: absolute;\s*top: 100%;[^}]*opacity: 0;/,
  );
  // Nothing under a message holds their place: no reserved height or margin anywhere.
  expect(chatMessageCss()).not.toMatch(/min-height: 1\.75rem|message-footer-row/);
  // The row in use comes forward over the next, and the last keeps room inside the list.
  expect(pointer).toMatch(
    /\[data-message-row\]:hover,\s*\[data-message-row\]:focus-within \{\s*z-index: 1;/,
  );
  expect(pointer).toMatch(/\[data-message-row\]\[data-last-message\] \{\s*padding-bottom: 2rem;/);
  // The bar keeps its own width, so under a short message its keys stay on one line.
  expect(pointer).toMatch(/\.message-actions-inline \{[^}]*width: max-content;/);
  const keys = lcdCss().match(
    /\[data-lcd-scope\] \[data-message-actions-inline\] \{([^}]*)\}/,
  )?.[1];
  expect(keys).toBeDefined();
  expect(keys).not.toMatch(/\bwidth:/);
  // Keys and their icons follow the text size, so the strip fits the gap at any size.
  expect(lcdCss()).toMatch(
    /\[data-message-actions-inline\] > button svg \{\s*width: 0\.875rem;\s*height: 0\.875rem;/,
  );
  // Without the bar, ⋮ ends the head line and a filtered view's Show in chat follows the time.
  expect(lcdCss()).toMatch(
    /\n:root\[data-ui-style="lcd"\] \.lcd-row \.message-actions-menu \{\s*margin-left: auto;/,
  );
  // ⋮ keeps inside the column, whose paint containment in a long list would cut it off.
  expect(lcdCss()).toMatch(
    /\.lcd-row \.message-actions-menu > button \{\s*justify-content: flex-end;\s*padding-right: 0;/,
  );
  expect(lcdCss()).not.toMatch(/message-actions-menu \{[^}]*margin:[^;]*-/);
  // One query decides where the bar shows, so at any width exactly one of the bar, or ⋮ with
  // the key after the time, offers the actions: a pointer and a column with room for the bar.
  const where =
    "@media (hover: hover) and (pointer: fine) {\n  @container messages (width >= 22rem) {";
  expect(pointer).toBeDefined();
  expect(chatMessageCss()).toContain(`${where}\n    .message-actions-menu {\n      display: none;`);
  expect(lcdCss().split(where)).toHaveLength(2);
  expect(lcdCss().split(where)[1]).toMatch(
    /^[^@]*:root\[data-ui-style="lcd"\] \.lcd-row-jump \{\s*display: none;/,
  );
});

it("outlines a jump target on the message box its row draws", async () => {
  await render(event, false, true);
  // Only the row draws the message box (its ::before, below the space above the
  // message), so the outline rule and the class must both be on the row.
  const rule = lcdCss().match(
    /:root\[data-ui-style="lcd"\] \.lcd-row\.lcd-row-highlighted::before \{([^}]*)\}/,
  );
  expect(rule?.[1]).toContain("dashed");
  expect(host.querySelector("[data-lcd-row]")?.matches(".lcd-row.lcd-row-highlighted")).toBe(true);
  expect(host.querySelector(".lcd-row-content.lcd-row-highlighted")).toBeNull();
});

it("separates messages with a line, without a ground or a hover tone", () => {
  const css = lcdCss();
  const box = css.match(/:root\[data-ui-style="lcd"\] \.lcd-row::before \{([^}]*)\}/);
  expect(box?.[1]).toBeDefined();
  expect(box?.[1]).not.toMatch(/background/);
  const line = css.match(
    /:root\[data-ui-style="lcd"\] \.lcd-row:not\(\[data-lcd-same-sender\]\)::after \{([^}]*)\}/,
  );
  expect(line?.[1]).toContain("border-top: 1px solid var(--lcd-sep)");
  // A tone change over a whole message read as a jump.
  expect(css).not.toMatch(/\.lcd-row:is\(:hover[^{]*::before/);
  expect(css).not.toMatch(/\.lcd-row:hover::before/);
});

it("draws a line where a sender's run starts, never inside one, as wide as the message below it", async () => {
  const css = lcdCss();
  // The line spans the row, and every row, yours included, is as wide as its text.
  const row = css.match(/:root\[data-ui-style="lcd"\] \.lcd-row \{([^}]*)\}/)?.[1];
  expect(row).toContain("width: fit-content");
  const line = css.match(/\.lcd-row:not\(\[data-lcd-same-sender\]\)::after \{([^}]*)\}/)?.[1];
  expect(line).toMatch(/left: 0;\s*right: 0;/);
  // No other rule draws, moves or hides a line, for your messages or a continued one.
  expect(css.match(/\.lcd-row[^{},]*::after/g)).toEqual([
    ".lcd-row:not([data-lcd-same-sender])::after",
  ]);
  // A question keeps room to be answered in, however short its text.
  expect(css).toMatch(/form\[data-pending-reply\] \{[^}]*width: 82ch;\s*max-width: 100%;/);
  // In a column wide enough, both sides keep the same room from the far edge; the
  // column, not the window, decides, so a side panel narrows it as a phone would.
  expect(css).toMatch(/\[data-message-column\] \{\s*container: messages \/ inline-size;/);
  expect(css).toMatch(
    /@container messages \(min-width: 36rem\) \{\s*:root\[data-ui-style="lcd"\] \.lcd-row,\s*:root\[data-ui-style="lcd"\] \.lcd-row\[data-lcd-user\] \{\s*max-width: calc\(100% - clamp\(6rem, 12%, 12rem\)\);\s*\}/,
  );

  await render(event, false, false, true);
  expect(host.querySelector("[data-lcd-row]")?.hasAttribute("data-lcd-same-sender")).toBe(true);
  await render();
  expect(host.querySelector("[data-lcd-row]")?.hasAttribute("data-lcd-same-sender")).toBe(false);
});

it("names you as the recipient of a message to you, and the user in a read-only exhibit", async () => {
  await render();
  expect(host.querySelector(".lcd-row-route [title]")?.getAttribute("title")).toBe(
    "messageAuthorYou",
  );
  await render(event, true);
  expect(host.querySelector(".lcd-row-route [title]")?.getAttribute("title")).toBe("senderUser");
});

it("sets the user's own messages on the reader's side, except in a read-only exhibit", async () => {
  const fromUser: LedgerEvent = { ...event, by: "user", data: { ...event.data, to: ["claude"] } };
  await render(fromUser);
  const row = host.querySelector("[data-lcd-row]");
  expect(row?.hasAttribute("data-lcd-user")).toBe(true);
  expect(host.querySelector(".lcd-row-name")?.textContent).toBe("messageAuthorYou");

  // Spectators of an exhibit are not the Group's user.
  await render(fromUser, true);
  expect(host.querySelector("[data-lcd-row]")?.hasAttribute("data-lcd-user")).toBe(false);
  expect(host.querySelector(".lcd-row-name")?.textContent).toBe("senderUser");
});
