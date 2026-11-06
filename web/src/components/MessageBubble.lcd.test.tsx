// @vitest-environment happy-dom
import { act } from "react";
import { readFileSync } from "node:fs";
import { fileURLToPath, URL as NodeURL } from "node:url";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, expect, it, vi } from "vite-plus/test";
import { MessageBubble } from "./MessageBubble";
import type { LedgerEvent } from "../types";
import { setUiStylePrefs } from "../utils/uiStyle";

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
  setUiStylePrefs({ style: "lcd" });
  host = document.createElement("div");
  document.body.appendChild(host);
  root = createRoot(host);
});

afterEach(() => {
  act(() => root.unmount());
  setUiStylePrefs({ style: "classic" });
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

it("keeps the pointer's actions and touch's ⋮ in the message's head", async () => {
  await render();
  const bar = host.querySelectorAll("[data-message-actions-inline]");
  expect(bar).toHaveLength(1);
  expect(bar[0].parentElement?.classList.contains("lcd-row-head")).toBe(true);
  expect(host.querySelector(".lcd-row-body [data-message-actions-inline]")).toBeNull();
  const menu = host.querySelectorAll(".message-actions-menu");
  expect(menu).toHaveLength(1);
  expect(menu[0].parentElement?.classList.contains("lcd-row-head")).toBe(true);
  expect(menu[0].previousElementSibling).toBe(bar[0]);
  expect(menu[0].nextElementSibling).toBeNull();
});

const lcdCss = () =>
  readFileSync(fileURLToPath(new NodeURL("../styles/lcd.css", import.meta.url)), "utf8");
const chatMessageCss = () =>
  readFileSync(fileURLToPath(new NodeURL("../styles/chatMessage.css", import.meta.url)), "utf8");

it("shows the pointer's actions beside the head, reserving nothing for them", () => {
  // One query decides where the bar shows, so at any width exactly one of the bar or ⋮ offers
  // the actions: a pointer, and a column with room for the bar beside a head.
  const where =
    "@media (hover: hover) and (pointer: fine) {\n  @container messages (width >= 28rem) {";
  expect(chatMessageCss()).toContain(
    `${where}\n    .message-actions-menu {\n      display: none;\n    }\n    .message-actions-inline {\n      display: flex;\n      opacity: 0;`,
  );
  const lcd = lcdCss().split(where);
  expect(lcd).toHaveLength(2);
  const placed = lcd[1];
  // A head is as wide as its text and stops short of the column's far edge; the bar sits in
  // the free space after it, centred on the head line, so showing it moves nothing.
  expect(placed).toMatch(
    /\.lcd-row-head \{\s*position: relative;\s*width: fit-content;\s*max-width: calc\(100cqi - 12\.5rem\);/,
  );
  expect(placed).toMatch(
    /\.lcd-row \.lcd-row-head > \[data-message-actions-inline\] \{\s*position: absolute;\s*top: 50%;\s*left: 100%;\s*width: max-content;\s*translate: 0 -50%;/,
  );
  // On your own messages it comes before your name.
  expect(placed).toMatch(
    /\.lcd-row\[data-lcd-user\]\s*\.lcd-row-head\s*> \[data-message-actions-inline\] \{\s*right: 100%;\s*left: auto;/,
  );
  // Nothing under a message holds or covers its place.
  expect(chatMessageCss()).not.toMatch(/data-last-message|top: 100%|message-footer-row/);
  // A filtered view's Show in chat leads the bar only where the column spares 19rem beside a
  // head; otherwise it follows the time, and the key after the time hides only there.
  expect(placed).toMatch(
    /@container messages \(width >= 36rem\) \{\s*:root\[data-ui-style="lcd"\] \.lcd-row\[data-lcd-show-in-chat\] \.lcd-row-head \{\s*max-width: calc\(100cqi - 19rem\);\s*\}\s*:root\[data-ui-style="lcd"\] \.lcd-row-jump \{\s*display: none;/,
  );
  expect(lcdCss().match(/\.lcd-row-jump \{\s*display: none;/g)).toHaveLength(1);
  // Without the bar, ⋮ ends the head line.
  expect(lcdCss()).toMatch(
    /\n:root\[data-ui-style="lcd"\] \.lcd-row \.message-actions-menu \{\s*margin-left: auto;/,
  );
  // ⋮ keeps inside the column, whose paint containment in a long list would cut it off.
  expect(lcdCss()).toMatch(
    /\.lcd-row \.message-actions-menu > button \{\s*justify-content: flex-end;\s*padding-right: 0;/,
  );
  expect(lcdCss()).not.toMatch(/message-actions-menu \{[^}]*margin:[^;]*-/);
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
