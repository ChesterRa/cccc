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
import * as pendingApi from "../services/api/pendingReplies";

vi.mock("react-i18next", () => ({ useTranslation: () => ({ t: (key: string) => key }) }));

Object.assign(globalThis, { IS_REACT_ACT_ENVIRONMENT: true });

// The store is read on the client, as in the app; a server render would see its initial state.
async function render(
  event: LedgerEvent,
  replyStore = true,
  inspect?: (host: HTMLDivElement) => Promise<void>,
) {
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
  try {
    if (inspect) await inspect(host);
    return host.innerHTML;
  } finally {
    await act(async () => root.unmount());
  }
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
  // The reply card marks the request, in place of the header's tag.
  const shown = await render(toUser);
  expect(shown).toContain("data-pending-reply");

  useUIStore.getState().setShowPendingPrompts(false);
  const off = await render(toUser);
  expect(off).toContain("Which path?");
  expect(off).not.toContain("needReply");
  expect(off).not.toContain("data-pending-reply");
});

it("leaves a request to the user unmarked while the reply store is still loading", async () => {
  useUIStore.getState().setShowPendingPrompts(false);
  expect(await render(toUser, false)).not.toContain("needReply");
  useUIStore.getState().setShowPendingPrompts(true);
  // Without a reply card, the header names the request instead.
  expect(await render(toUser, false)).toContain("needReply");
});

it("keeps the mark on a request between members while the indicators are off", async () => {
  useUIStore.getState().setShowPendingPrompts(false);
  const markup = await render(betweenMembers);
  expect(markup).toContain("needReply");
});

it("still submits an inline answer when only local navigation reminders are hidden", async () => {
  const original = useUIStore.getState();
  const stored = localStorage.getItem("cccc-show-pending-reply-reminders");
  const answer = vi
    .spyOn(pendingApi, "answerPendingReply")
    .mockResolvedValue({ ok: false, error: { code: "NETWORK_ERROR", message: "Fixture failure" } });
  try {
    useUIStore.getState().setShowPendingPrompts(true);
    useUIStore.getState().setShowPendingReplyReminders(false);
    await render(toUser, true, async (host) => {
      const input = host.querySelector<HTMLTextAreaElement>("textarea")!;
      await act(async () => {
        Object.getOwnPropertyDescriptor(HTMLTextAreaElement.prototype, "value")!.set!.call(
          input,
          "Fixture answer",
        );
        input.dispatchEvent(new Event("input", { bubbles: true }));
      });
      expect(host.querySelector<HTMLButtonElement>('button[type="submit"]')?.disabled).toBe(false);
      await act(async () =>
        host
          .querySelector("form")!
          .dispatchEvent(new Event("submit", { bubbles: true, cancelable: true })),
      );
      expect(answer).toHaveBeenCalledExactlyOnceWith("g_local", "to-user", expect.any(String), {
        v: 1,
        text: "Fixture answer",
      });
      expect(host.textContent).toContain("pendingReply.retrySame");
    });
  } finally {
    answer.mockRestore();
    useUIStore.setState(original);
    if (stored === null) localStorage.removeItem("cccc-show-pending-reply-reminders");
    else localStorage.setItem("cccc-show-pending-reply-reminders", stored);
  }
});
