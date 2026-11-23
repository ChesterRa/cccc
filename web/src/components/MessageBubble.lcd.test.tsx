// @vitest-environment happy-dom
import { act } from "react";
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

it("marks a highlighted jump target on its message row", async () => {
  await render(event, false, true);
  expect(host.querySelector("[data-lcd-row]")?.matches(".lcd-row.lcd-row-highlighted")).toBe(true);
  expect(host.querySelector(".lcd-row-content.lcd-row-highlighted")).toBeNull();
});

it("marks consecutive messages from the same sender on their rows", async () => {
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
