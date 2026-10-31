// @vitest-environment happy-dom
import { act } from "react";
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

async function render() {
  await act(async () =>
    root.render(
      <MessageBubble
        event={event}
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

it("keeps the classic actions in the footer", async () => {
  await render();
  expect(host.querySelectorAll("[data-message-actions-inline]")).toHaveLength(1);
  expect(host.querySelector(".lcd-row-head")).toBeNull();
});
