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
  host = document.createElement("div");
  document.body.appendChild(host);
  root = createRoot(host);
});

afterEach(() => {
  act(() => root.unmount());
  host.remove();
  setUiStylePrefs({ style: "classic" });
});

async function render(shown: LedgerEvent = event, readOnly = false, isHighlighted = false) {
  await act(async () =>
    root.render(
      <MessageBubble
        event={shown}
        readOnly={readOnly}
        isHighlighted={isHighlighted}
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
  setUiStylePrefs({ style: "lcd" });
  await render();
  const actions = host.querySelectorAll("[data-message-actions-inline]");
  expect(actions).toHaveLength(1);
  expect(actions[0].closest(".lcd-row-head")).not.toBeNull();
  expect(host.querySelector(".lcd-row-body [data-message-actions-inline]")).toBeNull();
});

const lcdCss = () =>
  readFileSync(fileURLToPath(new NodeURL("../styles/lcd.css", import.meta.url)), "utf8");

it("outlines a jump target on the message box its row draws", async () => {
  setUiStylePrefs({ style: "lcd" });
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
  const line = css.match(/:root\[data-ui-style="lcd"\] \.lcd-row::after \{([^}]*)\}/);
  expect(line?.[1]).toContain("border-top: 1px solid var(--lcd-sep)");
  // A tone change over a whole message read as a jump.
  expect(css).not.toMatch(/\.lcd-row:is\(:hover[^{]*::before/);
  expect(css).not.toMatch(/\.lcd-row:hover::before/);
});

it("keeps the classic actions in the footer", async () => {
  await render();
  expect(host.querySelectorAll("[data-message-actions-inline]")).toHaveLength(1);
  expect(host.querySelector(".lcd-row-head")).toBeNull();
});

it("sets the user's own messages on the reader's side, except in a read-only exhibit", async () => {
  setUiStylePrefs({ style: "lcd" });
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
