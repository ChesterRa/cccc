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

it("shows the message actions once, on the LCD head line", async () => {
  await render();
  const actions = host.querySelectorAll("[data-message-actions-inline]");
  expect(actions).toHaveLength(1);
  expect(actions[0].closest(".lcd-row-head")).not.toBeNull();
  expect(host.querySelector(".lcd-row-body [data-message-actions-inline]")).toBeNull();
});

const lcdCss = () =>
  readFileSync(fileURLToPath(new NodeURL("../styles/lcd.css", import.meta.url)), "utf8");

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

it("shows the desktop actions beside the head, so they never widen a message or its line", () => {
  const hover = lcdCss().match(
    /@media \(min-width: 640px\) and \(hover: hover\) \{([\s\S]*?)\n\}/,
  )?.[1];
  // Only where the column has room; a narrower one, as on a phone, puts every action in
  // the message's menu.
  expect(hover).toMatch(/^\s*@container messages \(min-width: 28rem\) \{/);
  const narrow = hover?.split("@container messages (width < 28rem) {")[1];
  expect(narrow).toMatch(/\.lcd-row \.message-actions-inline \{\s*display: none;/);
  expect(narrow).toMatch(/\.lcd-row \.message-actions-menu \{\s*display: block;/);
  // The head ends where its text does, and the actions sit just past that end; a head
  // stops short of the reading column's far edge by room for the widest set of actions.
  expect(hover).toMatch(
    /\.lcd-row-head \{[^}]*position: relative;[^}]*width: fit-content;[^}]*max-width: calc\(100cqi - 12\.5rem\);/,
  );
  expect(hover).toMatch(
    /\.lcd-row \.lcd-row-head > \[data-message-actions-inline\] \{[^}]*position: absolute;[^}]*left: 100%;/,
  );
  expect(hover).toMatch(
    /\.lcd-row\[data-lcd-user\]\s+\.lcd-row-head\s+> \[data-message-actions-inline\] \{[^}]*right: 100%;/,
  );
  // Beside a short message's head they sit past the message's own box, so the message's
  // whole band in the list keeps them showing while the pointer heads there from the text.
  const shown = readFileSync(
    fileURLToPath(new NodeURL("../styles/chatMessage.css", import.meta.url)),
    "utf8",
  );
  expect(shown).toMatch(
    /\[data-message-row\]:hover \.message-actions-inline,[^{]*\{\s*opacity: 1;\s*pointer-events: auto;/,
  );
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
