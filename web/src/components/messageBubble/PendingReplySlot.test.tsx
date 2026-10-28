// @vitest-environment happy-dom
import { act } from "react";
import { createRoot } from "react-dom/client";
import { expect, it, vi } from "vite-plus/test";
import { PendingSummaryContext } from "../../features/pendingReplies/context";
import {
  createPendingReplyStore,
  PendingReplyStoreContext,
  replyKey,
} from "../../features/pendingReplies/store";
import type { LedgerEvent } from "../../types";
import { PendingReplySlot } from "./PendingReplySlot";

const mocks = vi.hoisted(() => ({
  answer: vi.fn(),
  append: vi.fn(),
  update: vi.fn(),
  groups: {} as Record<string, { events: LedgerEvent[] }>,
}));
vi.mock("../../services/api/pendingReplies", () => ({ answerPendingReply: mocks.answer }));
vi.mock("react-i18next", () => ({
  useTranslation: () => ({
    t: (key: string, values?: { choice?: string }) =>
      values?.choice ? `${key}: ${values.choice}` : key,
  }),
}));
vi.mock("../../stores", () => ({
  useGroupStore: Object.assign(
    (select: (state: { chatByGroup: typeof mocks.groups }) => unknown) =>
      select({ chatByGroup: mocks.groups }),
    { getState: () => ({ appendEvent: mocks.append, updateObligationStatus: mocks.update }) },
  ),
}));

const event: LedgerEvent = {
  id: "r1",
  group_id: "g1",
  kind: "chat.message",
  by: "peer",
  data: {
    to: ["user"],
    message_mode: "request_reply",
    text: "Question",
    question: {
      v: 1,
      prompt: "Question",
      options: [
        { id: "A", label: "Alpha" },
        { id: "B", label: "Beta" },
      ],
    },
  },
  _obligation_status: {
    user: { reply_requested: true, replied: false, cancelled: false, delivery_state: "" },
  },
  _user_request: { question: "valid", requester: "available" },
};

it("retains an uncertain answer and retries the identical request before marking it answered", async () => {
  (globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;
  mocks.answer.mockReset();
  mocks.append.mockReset();
  mocks.update.mockReset();
  const host = document.createElement("div");
  document.body.append(host);
  const root = createRoot(host);
  const store = createPendingReplyStore();
  const refresh = vi.fn();
  mocks.answer.mockResolvedValueOnce({ ok: false, error: { code: "NETWORK_ERROR" } });
  let finishRetry!: () => void;
  mocks.answer.mockImplementationOnce(
    (_group, _source, clientId) =>
      new Promise((resolve) => {
        finishRetry = () =>
          resolve({
            ok: true,
            result: {
              event: {
                id: "response",
                group_id: "g1",
                by: "user",
                kind: "chat.message",
                data: {
                  reply_to: "r1",
                  to: ["peer"],
                  client_id: clientId,
                  message_mode: "send",
                  answer: { v: 1, option_id: "A" },
                  text: "Selected A: Alpha",
                },
              },
            },
          });
      }),
  );
  try {
    await act(async () =>
      root.render(
        <PendingSummaryContext.Provider value={{ summaries: {}, refresh }}>
          <PendingReplyStoreContext.Provider value={store}>
            <PendingReplySlot event={event} groupId="g1" onUseComposer={() => {}} />
          </PendingReplyStoreContext.Provider>
        </PendingSummaryContext.Provider>,
      ),
    );
    const choice = host.querySelector<HTMLInputElement>('input[type="radio"]')!;
    await act(async () => choice.click());
    await act(async () =>
      host
        .querySelector<HTMLFormElement>("form")!
        .dispatchEvent(new Event("submit", { bubbles: true, cancelable: true })),
    );
    const first = mocks.answer.mock.calls[0];
    expect(store.getState().entries[replyKey("g1", "r1")].attempt?.phase).toBe("uncertain");
    expect(choice.disabled).toBe(true);
    expect(mocks.append).not.toHaveBeenCalled();
    // A failed status refresh loses the current projection, but the retry
    // must retain the question as it was when the answer was submitted.
    await act(async () =>
      root.render(
        <PendingSummaryContext.Provider value={{ summaries: {}, refresh }}>
          <PendingReplyStoreContext.Provider value={store}>
            <PendingReplySlot
              event={{ ...event, _obligation_status: undefined, _user_request: undefined }}
              groupId="g1"
              onUseComposer={() => {}}
            />
          </PendingReplyStoreContext.Provider>
        </PendingSummaryContext.Provider>,
      ),
    );
    const retry = host.querySelector<HTMLButtonElement>('button[type="button"]')!;
    expect(retry).not.toBeNull();
    await act(async () => retry.click());
    expect(mocks.answer.mock.calls[1]).toEqual(first);
    expect(host.querySelector("form")).not.toBeNull();
    expect(store.getState().entries[replyKey("g1", "r1")].attempt?.phase).toBe("sending");
    // Another status read can see closure before this client's retry returns.
    await act(async () =>
      root.render(
        <PendingSummaryContext.Provider value={{ summaries: {}, refresh }}>
          <PendingReplyStoreContext.Provider value={store}>
            <PendingReplySlot
              event={{
                ...event,
                _obligation_status: {
                  user: {
                    reply_requested: true,
                    replied: true,
                    cancelled: false,
                    delivery_state: "",
                  },
                },
              }}
              groupId="g1"
              onUseComposer={() => {}}
            />
          </PendingReplyStoreContext.Provider>
        </PendingSummaryContext.Provider>,
      ),
    );
    await act(async () => finishRetry());
    expect(store.getState().entries[replyKey("g1", "r1")].closed).toBe("answered");
    expect(host.textContent).toContain("A. Alpha");
    expect(mocks.append).toHaveBeenCalledOnce();
    expect(mocks.update).toHaveBeenCalledOnce();
    expect(refresh).toHaveBeenCalledOnce();
  } finally {
    await act(async () => root.unmount());
    host.remove();
  }
});

it("shows a loaded recorded choice, but never labels a mismatched reply as an answer", async () => {
  (globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;
  const host = document.createElement("div");
  document.body.append(host);
  const root = createRoot(host);
  const store = createPendingReplyStore();
  const closed = {
    ...event,
    _obligation_status: {
      user: { reply_requested: true, replied: true, cancelled: false, delivery_state: "" },
    },
    _user_request: {
      question: "valid" as const,
      requester: "available" as const,
      reply_event_id: "a1",
    },
  };
  const reply: LedgerEvent = {
    id: "a1",
    kind: "chat.message",
    by: "user",
    data: {
      reply_to: "r1",
      to: ["peer"],
      message_mode: "send",
      answer: { v: 1, option_id: "B" },
      text: "Selected B: Beta",
    },
  };
  const render = () =>
    act(async () =>
      root.render(
        <PendingReplyStoreContext.Provider value={store}>
          <PendingReplySlot event={closed} groupId="g1" onUseComposer={() => {}} />
        </PendingReplyStoreContext.Provider>,
      ),
    );
  try {
    mocks.groups.g1 = { events: [reply] };
    await render();
    expect(host.textContent).toContain("B. Beta");
    mocks.groups.g1 = { events: [{ ...reply, data: { ...reply.data, text: "Different answer" } }] };
    await render();
    expect(host.textContent).not.toContain("B. Beta");
  } finally {
    mocks.groups = {};
    await act(async () => root.unmount());
    host.remove();
  }
});

it("keeps an answer uncertain if the success receipt describes a different answer", async () => {
  (globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;
  mocks.answer.mockReset();
  mocks.answer.mockImplementationOnce((_group, _source, clientId) =>
    Promise.resolve({
      ok: true,
      result: {
        event: {
          id: "a1",
          group_id: "g1",
          kind: "chat.message",
          by: "user",
          data: {
            client_id: clientId,
            reply_to: "r1",
            to: ["peer"],
            message_mode: "send",
            answer: { v: 1, option_id: "B" },
            text: "Selected B: Beta",
          },
        },
      },
    }),
  );
  const host = document.createElement("div");
  document.body.append(host);
  const root = createRoot(host);
  const store = createPendingReplyStore();
  try {
    await act(async () =>
      root.render(
        <PendingReplyStoreContext.Provider value={store}>
          <PendingReplySlot event={event} groupId="g1" onUseComposer={() => {}} />
        </PendingReplyStoreContext.Provider>,
      ),
    );
    await act(async () => host.querySelector<HTMLInputElement>('input[type="radio"]')!.click());
    await act(async () =>
      host
        .querySelector<HTMLFormElement>("form")!
        .dispatchEvent(new Event("submit", { bubbles: true, cancelable: true })),
    );
    expect(store.getState().entries[replyKey("g1", "r1")].attempt?.phase).toBe("uncertain");
    expect(store.getState().entries[replyKey("g1", "r1")].confirmed).toBe(false);
  } finally {
    await act(async () => root.unmount());
    host.remove();
  }
});

it("ignores a completion after the authenticated scope has been removed", async () => {
  (globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;
  mocks.answer.mockReset();
  mocks.append.mockReset();
  mocks.update.mockReset();
  const host = document.createElement("div");
  document.body.append(host);
  const root = createRoot(host);
  const store = createPendingReplyStore();
  let finish!: (value: unknown) => void;
  mocks.answer.mockReturnValueOnce(
    new Promise((resolve) => {
      finish = resolve;
    }),
  );
  try {
    await act(async () =>
      root.render(
        <PendingReplyStoreContext.Provider value={store}>
          <PendingReplySlot event={event} groupId="g1" onUseComposer={() => {}} />
        </PendingReplyStoreContext.Provider>,
      ),
    );
    await act(async () => host.querySelector<HTMLInputElement>('input[type="radio"]')!.click());
    await act(async () =>
      host
        .querySelector<HTMLFormElement>("form")!
        .dispatchEvent(new Event("submit", { bubbles: true, cancelable: true })),
    );
    const clientId = mocks.answer.mock.calls[0][2];
    await act(async () => store.getState().retainGroups([]));
    await act(async () =>
      finish({
        ok: true,
        result: {
          event: {
            id: "response",
            group_id: "g1",
            by: "user",
            data: { reply_to: "r1", client_id: clientId },
          },
        },
      }),
    );
    expect(mocks.append).not.toHaveBeenCalled();
    expect(mocks.update).not.toHaveBeenCalled();
  } finally {
    await act(async () => root.unmount());
    host.remove();
  }
});

it.each([
  ["invalid_source_event", true],
  ["requester_unavailable", true],
  ["requester_unknown", true],
  ["unsupported_request_source", true],
  ["question_unavailable", true],
  ["invalid_args", false],
  ["invalid_answer", false],
  ["permission_denied", false],
  ["idempotency_conflict", false],
  ["idempotency_unavailable", false],
  ["ledger_unreadable", false],
])("resolves an uncertain attempt on %s only when the lookup finished", async (code, resolves) => {
  (globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;
  mocks.answer.mockReset();
  mocks.answer.mockResolvedValueOnce({ ok: false, error: { code: "NETWORK_ERROR" } });
  mocks.answer.mockResolvedValueOnce({ ok: false, error: { code } });
  const host = document.createElement("div");
  document.body.append(host);
  const root = createRoot(host);
  const store = createPendingReplyStore();
  try {
    await act(async () =>
      root.render(
        <PendingReplyStoreContext.Provider value={store}>
          <PendingReplySlot event={event} groupId="g1" onUseComposer={() => {}} />
        </PendingReplyStoreContext.Provider>,
      ),
    );
    await act(async () => host.querySelector<HTMLInputElement>('input[type="radio"]')!.click());
    await act(async () =>
      host
        .querySelector<HTMLFormElement>("form")!
        .dispatchEvent(new Event("submit", { bubbles: true, cancelable: true })),
    );
    await act(async () => host.querySelector<HTMLButtonElement>('button[type="button"]')!.click());
    const entry = store.getState().entries[replyKey("g1", "r1")];
    expect(entry.attempt?.phase ?? null).toBe(resolves ? null : "uncertain");
    expect(entry.draft.selection).toEqual({ kind: "option", id: "A" });
  } finally {
    await act(async () => root.unmount());
    host.remove();
  }
});
