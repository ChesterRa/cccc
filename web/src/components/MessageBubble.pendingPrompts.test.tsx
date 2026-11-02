// @vitest-environment happy-dom
import { act } from "react";
import { createRoot } from "react-dom/client";
import { afterEach, expect, it, vi } from "vite-plus/test";
import { MessageBubble } from "./MessageBubble";
import {
  createPendingReplyStore,
  PendingReplyStoreContext,
} from "../features/pendingReplies/store";
import { useUIStore } from "../stores/useUIStore";
import type { LedgerEvent } from "../types";
import { setUiStylePrefs } from "../utils/uiStyle";

vi.mock("react-i18next", () => ({ useTranslation: () => ({ t: (key: string) => key }) }));

Object.assign(globalThis, { IS_REACT_ACT_ENVIRONMENT: true });

// The store is read on the client, as in the app; a server render would see its initial state.
async function render(event: LedgerEvent, replyStore = true) {
  const host = document.createElement("div");
  const root = createRoot(host);
  await act(async () =>
    root.render(
      <PendingReplyStoreContext.Provider value={replyStore ? createPendingReplyStore() : null}>
        <MessageBubble
          event={event}
          actorById={new Map()}
          actors={[]}
          displayNameMap={new Map()}
          agentState={null}
          taskById={new Map()}
          isDark={false}
          groupId="g_local"
          groupLabelById={{}}
          onReply={() => undefined}
          onShowRecipients={() => undefined}
          onOpenSource={() => undefined}
        />
      </PendingReplyStoreContext.Provider>,
    ),
  );
  const markup = host.innerHTML;
  await act(async () => root.unmount());
  return markup;
}

const toUser: LedgerEvent = {
  id: "to-user",
  kind: "chat.message",
  by: "worker",
  data: { message_mode: "request_reply", to: ["user"], text: "Which path?" },
  _obligation_status: {
    user: { reply_requested: true, replied: false, cancelled: false, delivery_state: "" },
  },
  _user_request: { question: "none", requester: "available" },
};

const betweenMembers: LedgerEvent = {
  id: "to-worker",
  kind: "chat.message",
  by: "lead",
  data: { message_mode: "request_reply", to: ["worker"], text: "Report back." },
};

afterEach(() => {
  useUIStore.setState({ showPendingPrompts: true });
  localStorage.removeItem("cccc-pending-prompts");
});

it("marks a request to the user only while the 'Waiting for you' indicators are shown", async () => {
  const shown = await render(toUser);
  expect(shown).toContain("needReply");
  expect(shown).toContain("ring-violet");

  useUIStore.getState().setShowPendingPrompts(false);
  const off = await render(toUser);
  expect(off).toContain("Which path?");
  expect(off).not.toContain("needReply");
  expect(off).not.toContain("ring-violet");
  expect(off).not.toContain("data-pending-reply");
});

it("leaves a request to the user unmarked while the reply store is still loading", async () => {
  useUIStore.getState().setShowPendingPrompts(false);
  expect(await render(toUser, false)).not.toContain("needReply");
  try {
    await act(async () => setUiStylePrefs({ style: "lcd" }));
    expect(await render(toUser, false)).not.toContain("needReply");
    useUIStore.getState().setShowPendingPrompts(true);
    // Without a reply card, LCD names the request in its header instead.
    expect(await render(toUser, false)).toContain("needReply");
  } finally {
    await act(async () => setUiStylePrefs({ style: "classic" }));
  }
});

it("keeps the mark on a request between members while the indicators are off", async () => {
  useUIStore.getState().setShowPendingPrompts(false);
  const markup = await render(betweenMembers);
  expect(markup).toContain("needReply");
  expect(markup).toContain("ring-violet");
});
