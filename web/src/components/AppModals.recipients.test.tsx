// @vitest-environment happy-dom
import { act, createRef } from "react";
import { createRoot } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, it, vi } from "vite-plus/test";
import type { Actor, GroupDoc, LedgerEvent, LedgerEventStatusPayload } from "../types";
import type { RecipientsModalProps } from "./modals/RecipientsModal";

const mocks = vi.hoisted(() => ({
  fetchLedgerStatuses: vi.fn(),
  deliverMessage: vi.fn(),
  cancelReplyRequest: vi.fn(),
  recipientsProps: vi.fn(),
  t: (key: string) => key,
}));

vi.mock("react-i18next", async (original) => ({
  ...(await original<typeof import("react-i18next")>()),
  useTranslation: () => ({ t: mocks.t }),
}));
vi.mock("../services/api", () => ({
  fetchGroupPrompts: async () => ({ ok: true, result: { help: { content: "" } } }),
  listActorProfiles: async () => ({ ok: true, result: { profiles: [] } }),
  fetchLedgerStatuses: (...args: unknown[]) => mocks.fetchLedgerStatuses(...args),
  deliverMessage: (...args: unknown[]) => mocks.deliverMessage(...args),
  cancelReplyRequest: (...args: unknown[]) => mocks.cancelReplyRequest(...args),
}));
vi.mock("./modals/ActorConfigModal", () => ({ ActorConfigModal: () => null }));
vi.mock("./SearchModal", () => ({ SearchModal: () => null }));
vi.mock("./layout/MobileMenuSheet", () => ({ MobileMenuSheet: () => null }));
vi.mock("./modals/CreateGroupModal", () => ({ CreateGroupModal: () => null }));
vi.mock("./modals/GroupEditModal", () => ({ GroupEditModal: () => null }));
vi.mock("./modals/InboxModal", () => ({ InboxModal: () => null }));
vi.mock("./presentation/PresentationPinModal", () => ({ PresentationPinModal: () => null }));
vi.mock("./modals/RelayMessageModal", () => ({ RelayMessageModal: () => null }));
vi.mock("./modals/RecipientsModal", () => ({
  RecipientsModal: (props: RecipientsModalProps) => {
    mocks.recipientsProps(props);
    return null;
  },
}));

import { AppModals } from "./AppModals";
import { i18nReady } from "../i18n";
import { useGroupStore, useModalStore, useUIStore } from "../stores";

function deferred<T>() {
  let resolve!: (value: T) => void;
  const promise = new Promise<T>((r) => {
    resolve = r;
  });
  return { promise, resolve };
}

function workerActor(overrides: Partial<Actor> = {}): Actor {
  return {
    id: "worker",
    role: "peer",
    enabled: false,
    running: false,
    runtime: "codex",
    ...overrides,
  };
}

function sendEvent(waitingReason?: string): LedgerEvent {
  return {
    id: "evt-send-1",
    kind: "chat.message",
    by: "user",
    ts: "2026-11-22T12:00:00Z",
    data: { text: "Please check", to: ["worker"], message_mode: "send" },
    _obligation_status: {
      worker: {
        replied: false,
        reply_requested: false,
        cancelled: false,
        delivery_state: "",
        ...(waitingReason ? { waiting_reason: waitingReason } : {}),
      },
    },
  };
}

function statusResponse(status: LedgerEventStatusPayload) {
  return { ok: true as const, result: { statuses: { "evt-send-1": status } } };
}

let host: HTMLDivElement;
let root: ReturnType<typeof createRoot>;

async function renderModals() {
  await act(async () =>
    root.render(
      <AppModals
        codexVoice={{} as React.ComponentProps<typeof AppModals>["codexVoice"]}
        isDark={false}
        theme="light"
        textScale={100}
        ccccHome="/fixture-only"
        composerRef={createRef()}
        onStartReply={vi.fn()}
        onNativeActorCreated={vi.fn()}
        onThemeChange={vi.fn()}
        onTextScaleChange={vi.fn()}
        onDeleteGroup={vi.fn()}
        fetchContext={vi.fn()}
        canManageGroups
      />,
    ),
  );
}

function latestRecipientsProps(): RecipientsModalProps {
  const calls = mocks.recipientsProps.mock.calls;
  expect(calls.length).toBeGreaterThan(0);
  return calls[calls.length - 1][0] as RecipientsModalProps;
}

describe("AppModals RecipientsModal status refresh", () => {
  beforeEach(async () => {
    Object.assign(globalThis, { IS_REACT_ACT_ENVIRONMENT: true });
    vi.clearAllMocks();
    mocks.fetchLedgerStatuses.mockResolvedValue({ ok: true, result: { statuses: {} } });
    mocks.deliverMessage.mockResolvedValue({ ok: true, result: {} });
    mocks.cancelReplyRequest.mockResolvedValue({ ok: true, result: {} });
    useModalStore.setState({
      modals: { ...useModalStore.getState().modals },
      recipientsEventId: null,
      editingActor: null,
    });
    useUIStore.setState({ activeTab: "chat", chatSessions: {}, busy: "", isSmallScreen: false });
    useGroupStore.setState({
      selectedGroupId: "g_a",
      groupDoc: { group_id: "g_a", state: "paused" } as GroupDoc,
      groups: [{ group_id: "g_a", state: "paused" }],
      actors: [workerActor({ enabled: false })],
      runtimes: [],
      chatByGroup: {},
    });
    host = document.createElement("div");
    document.body.append(host);
    root = createRoot(host);
    await renderModals();
  });

  afterEach(async () => {
    await act(async () => root.unmount());
    host.remove();
    useModalStore.setState({ recipientsEventId: null });
    useUIStore.getState().dismissError();
  });

  it("refreshes single-event ledger_statuses on open for SSE send messages and on group resume", async () => {
    mocks.fetchLedgerStatuses
      .mockResolvedValueOnce(
        statusResponse({
          obligation_status: {
            worker: {
              replied: false,
              reply_requested: false,
              cancelled: false,
              delivery_state: "",
              waiting_reason: "group_paused",
            },
          },
        }),
      )
      .mockResolvedValueOnce(
        statusResponse({
          obligation_status: {
            worker: {
              replied: false,
              reply_requested: false,
              cancelled: false,
              delivery_state: "",
              waiting_reason: "member_disabled",
            },
          },
        }),
      );

    await act(async () => {
      useGroupStore.getState().setEvents([sendEvent()], "g_a");
      useModalStore.getState().setRecipientsModal("evt-send-1");
    });

    expect(mocks.fetchLedgerStatuses).toHaveBeenCalledTimes(1);
    expect(mocks.fetchLedgerStatuses).toHaveBeenLastCalledWith(
      "g_a",
      ["evt-send-1"],
      expect.objectContaining({ noCache: true, signal: expect.any(AbortSignal) }),
    );
    expect(latestRecipientsProps().entries[0]?.waitingReason).toBe("group_paused");

    // Resuming the Group while the recipient actor remains disabled must trigger a fresh read.
    await act(async () => {
      useGroupStore.setState({
        groupDoc: { group_id: "g_a", state: "active" } as GroupDoc,
        groups: [{ group_id: "g_a", state: "active" }],
      });
    });

    expect(mocks.fetchLedgerStatuses).toHaveBeenCalledTimes(2);
    expect(latestRecipientsProps().entries[0]?.waitingReason).toBe("member_disabled");
  });

  it("does not overwrite a newer live SSE delivery receipt with a late open-refresh snapshot", async () => {
    const slowRefresh = deferred<ReturnType<typeof statusResponse>>();
    mocks.fetchLedgerStatuses.mockImplementationOnce(() => slowRefresh.promise);

    await act(async () => {
      useGroupStore.getState().setEvents([sendEvent("group_paused")], "g_a");
      useModalStore.getState().setRecipientsModal("evt-send-1");
    });

    await act(async () => {
      useGroupStore
        .getState()
        .updateObligationStatus(
          "evt-send-1",
          {
            actorId: "worker",
            deliveryState: "accepted",
            deliveryReason: "",
            deliveryEvidence: "terminal_written",
            waitingReason: undefined,
          },
          "g_a",
        );
    });

    await act(async () => {
      slowRefresh.resolve(
        statusResponse({
          obligation_status: {
            worker: {
              replied: false,
              reply_requested: false,
              cancelled: false,
              delivery_state: "",
              waiting_reason: "group_paused",
            },
          },
        }),
      );
      await slowRefresh.promise;
    });

    expect(latestRecipientsProps().entries[0]).toMatchObject({
      deliveryState: "accepted",
      deliveryEvidence: "terminal_written",
      waitingReason: undefined,
    });
  });

  it("aborts superseded refresh requests and ignores out-of-order late responses", async () => {
    const firstRefresh = deferred<ReturnType<typeof statusResponse>>();
    const secondRefresh = deferred<ReturnType<typeof statusResponse>>();
    const signals: AbortSignal[] = [];
    mocks.fetchLedgerStatuses
      .mockImplementationOnce((_gid, _ids, init: { signal: AbortSignal }) => {
        signals.push(init.signal);
        return firstRefresh.promise;
      })
      .mockImplementationOnce((_gid, _ids, init: { signal: AbortSignal }) => {
        signals.push(init.signal);
        return secondRefresh.promise;
      });

    await act(async () => {
      useGroupStore.getState().setEvents([sendEvent("group_paused")], "g_a");
      useModalStore.getState().setRecipientsModal("evt-send-1");
    });
    expect(signals[0]?.aborted).toBe(false);

    await act(async () => {
      useGroupStore.setState({
        groupDoc: { group_id: "g_a", state: "active" } as GroupDoc,
        groups: [{ group_id: "g_a", state: "active" }],
      });
    });
    expect(signals[0]?.aborted).toBe(true);
    expect(signals[1]?.aborted).toBe(false);

    await act(async () => {
      secondRefresh.resolve(
        statusResponse({
          obligation_status: {
            worker: {
              replied: false,
              reply_requested: false,
              cancelled: false,
              delivery_state: "",
              waiting_reason: "member_disabled",
            },
          },
        }),
      );
      await secondRefresh.promise;
    });
    expect(latestRecipientsProps().entries[0]?.waitingReason).toBe("member_disabled");

    // Resolving the aborted first request late must not restore the stale reason.
    await act(async () => {
      firstRefresh.resolve(
        statusResponse({
          obligation_status: {
            worker: {
              replied: false,
              reply_requested: false,
              cancelled: false,
              delivery_state: "",
              waiting_reason: "group_paused",
            },
          },
        }),
      );
      await firstRefresh.promise;
    });
    expect(latestRecipientsProps().entries[0]?.waitingReason).toBe("member_disabled");
  });

  it("aborts in-flight refresh on modal close and cancels passive refresh before manual Send now", async () => {
    const openRefresh = deferred<ReturnType<typeof statusResponse>>();
    const deliverRefresh = deferred<ReturnType<typeof statusResponse>>();
    const signals: AbortSignal[] = [];
    mocks.fetchLedgerStatuses
      .mockImplementationOnce((_gid, _ids, init: { signal: AbortSignal }) => {
        signals.push(init.signal);
        return openRefresh.promise;
      })
      .mockImplementationOnce((_gid, _ids, init: { signal: AbortSignal }) => {
        signals.push(init.signal);
        return deliverRefresh.promise;
      });

    await act(async () => {
      useGroupStore.setState({ actors: [workerActor({ enabled: true })] });
      useGroupStore.getState().setEvents([sendEvent("group_paused")], "g_a");
      useModalStore.getState().setRecipientsModal("evt-send-1");
    });
    expect(signals[0]?.aborted).toBe(false);

    await act(async () => {
      latestRecipientsProps().onDeliver("worker", false);
      await Promise.resolve();
    });
    expect(signals[0]?.aborted).toBe(true);
    expect(mocks.deliverMessage).toHaveBeenCalledWith("g_a", "evt-send-1", ["worker"], false);

    await act(async () => {
      deliverRefresh.resolve(
        statusResponse({
          obligation_status: {
            worker: {
              replied: false,
              reply_requested: false,
              cancelled: false,
              delivery_state: "accepted",
              delivery_evidence: "channel_accepted",
            },
          },
        }),
      );
      await deliverRefresh.promise;
    });
    await act(async () => {
      openRefresh.resolve(
        statusResponse({
          obligation_status: {
            worker: {
              replied: false,
              reply_requested: false,
              cancelled: false,
              delivery_state: "",
              waiting_reason: "group_paused",
            },
          },
        }),
      );
      await openRefresh.promise;
    });

    expect(latestRecipientsProps().entries[0]).toMatchObject({
      deliveryState: "accepted",
      deliveryEvidence: "channel_accepted",
      waitingReason: undefined,
    });

    // Closing the modal aborts any newly started in-flight refresh.
    const thirdRefresh = deferred<ReturnType<typeof statusResponse>>();
    mocks.fetchLedgerStatuses.mockImplementationOnce(
      (_gid, _ids, init: { signal: AbortSignal }) => {
        signals.push(init.signal);
        return thirdRefresh.promise;
      },
    );
    await act(async () => {
      useGroupStore.setState({
        groupDoc: { group_id: "g_a", state: "active" } as GroupDoc,
        groups: [{ group_id: "g_a", state: "active" }],
      });
    });
    expect(signals[2]?.aborted).toBe(false);
    await act(async () => useModalStore.getState().setRecipientsModal(null));
    expect(signals[2]?.aborted).toBe(true);
  });

  it("does not abort B's refresh when A's pending deliver or cancel resolves after switching to B", async () => {
    const eventA: LedgerEvent = {
      ...sendEvent("group_paused"),
      id: "evt-req-a",
      data: { text: "Message A", to: ["worker"], message_mode: "request_reply" },
      _obligation_status: {
        worker: {
          replied: false,
          reply_requested: true,
          cancelled: false,
          delivery_state: "",
          waiting_reason: "group_paused",
        },
      },
    };
    const eventB: LedgerEvent = {
      ...sendEvent(),
      id: "evt-send-b",
      data: { text: "Message B", to: ["worker"], message_mode: "send" },
    };

    const deliverA = deferred<{ ok: true; result: Record<string, unknown> }>();
    const cancelA = deferred<{ ok: true; result: Record<string, unknown> }>();
    const refreshB1 = deferred<{
      ok: true;
      result: { statuses: Record<string, LedgerEventStatusPayload> };
    }>();
    const refreshB2 = deferred<{
      ok: true;
      result: { statuses: Record<string, LedgerEventStatusPayload> };
    }>();
    const bSignals: AbortSignal[] = [];

    mocks.deliverMessage.mockImplementationOnce(() => deliverA.promise);
    mocks.cancelReplyRequest.mockImplementationOnce(() => cancelA.promise);
    mocks.fetchLedgerStatuses
      .mockResolvedValueOnce({ ok: true, result: { statuses: {} } })
      .mockImplementationOnce((_gid, _ids, init: { signal: AbortSignal }) => {
        bSignals.push(init.signal);
        return refreshB1.promise;
      })
      .mockResolvedValueOnce({ ok: true, result: { statuses: {} } })
      .mockImplementationOnce((_gid, _ids, init: { signal: AbortSignal }) => {
        bSignals.push(init.signal);
        return refreshB2.promise;
      });

    await act(async () => {
      useGroupStore.setState({ actors: [workerActor({ enabled: true })] });
      useGroupStore.getState().setEvents([eventA, eventB], "g_a");
      useModalStore.getState().setRecipientsModal("evt-req-a");
    });
    expect(mocks.fetchLedgerStatuses).toHaveBeenCalledTimes(1);

    // 1. A starts manual delivery (pending), user closes A and opens B before A finishes.
    await act(async () => {
      latestRecipientsProps().onDeliver("worker", false);
    });
    await act(async () => {
      useModalStore.getState().setRecipientsModal("evt-send-b");
    });
    expect(mocks.fetchLedgerStatuses).toHaveBeenCalledTimes(2);
    expect(mocks.fetchLedgerStatuses).toHaveBeenLastCalledWith(
      "g_a",
      ["evt-send-b"],
      expect.objectContaining({ noCache: true, signal: expect.any(AbortSignal) }),
    );
    expect(bSignals[0]?.aborted).toBe(false);

    // A's deliver resolves while B is open: must NOT abort B or refetch A.
    await act(async () => {
      deliverA.resolve({ ok: true, result: {} });
      await deliverA.promise;
    });
    expect(bSignals[0]?.aborted).toBe(false);
    expect(mocks.fetchLedgerStatuses).toHaveBeenCalledTimes(2);

    await act(async () => {
      refreshB1.resolve({
        ok: true,
        result: {
          statuses: {
            "evt-send-b": {
              obligation_status: {
                worker: {
                  replied: false,
                  reply_requested: false,
                  cancelled: false,
                  delivery_state: "",
                  waiting_reason: "member_disabled",
                },
              },
            },
          },
        },
      });
      await refreshB1.promise;
    });
    expect(latestRecipientsProps().canCancelReply).toBe(false);
    expect(latestRecipientsProps().entries[0]?.waitingReason).toBe("member_disabled");

    // 2. Same race guard for cancelReplyRequest: switch back to A, start cancel, switch to B, finish A.
    await act(async () => {
      useModalStore.getState().setRecipientsModal("evt-req-a");
    });
    await act(async () => {
      latestRecipientsProps().onCancelReply();
    });
    await act(async () => {
      useModalStore.getState().setRecipientsModal("evt-send-b");
    });
    expect(mocks.fetchLedgerStatuses).toHaveBeenCalledTimes(4);
    expect(bSignals[1]?.aborted).toBe(false);

    await act(async () => {
      cancelA.resolve({ ok: true, result: {} });
      await cancelA.promise;
    });
    expect(bSignals[1]?.aborted).toBe(false);
    expect(mocks.fetchLedgerStatuses).toHaveBeenCalledTimes(4);

    await act(async () => {
      refreshB2.resolve({
        ok: true,
        result: {
          statuses: {
            "evt-send-b": {
              obligation_status: {
                worker: {
                  replied: false,
                  reply_requested: false,
                  cancelled: false,
                  delivery_state: "",
                  waiting_reason: "group_paused",
                },
              },
            },
          },
        },
      });
      await refreshB2.promise;
    });
    expect(latestRecipientsProps().entries[0]?.waitingReason).toBe("group_paused");
  });

  it("maps actionable Send Now errors to localized text with original code and keeps unmapped errors raw", async () => {
    await i18nReady;
    await act(async () => {
      useGroupStore.setState({ actors: [workerActor({ enabled: true })] });
      useGroupStore.getState().setEvents([sendEvent()], "g_a");
      useModalStore.getState().setRecipientsModal("evt-send-1");
    });

    mocks.deliverMessage.mockResolvedValueOnce({
      ok: false,
      error: {
        code: "delivery_in_progress",
        message: "delivery is already in progress for actor: worker",
      },
    });
    await act(async () => {
      await latestRecipientsProps().onDeliver("worker", false);
    });
    expect(useUIStore.getState().errorMsg).toBe(
      "Delivery is already in progress. No need to send again; recipient status will update when it finishes (delivery_in_progress).",
    );

    mocks.deliverMessage.mockResolvedValueOnce({
      ok: false,
      error: {
        code: "delivery_ambiguous",
        message: "delivery may already have occurred for actor: worker",
      },
    });
    await act(async () => {
      await latestRecipientsProps().onDeliver("worker", false);
    });
    expect(useUIStore.getState().errorMsg).toBe(
      'Delivery may already have occurred. Check the member\'s terminal or wait for a reply; if you need to send it again, reopen recipient status and use "Retry anyway" (delivery_ambiguous).',
    );

    mocks.deliverMessage.mockResolvedValueOnce({
      ok: false,
      error: {
        code: "already_delivered",
        message: "message was already accepted for actor: worker",
      },
    });
    await act(async () => {
      await latestRecipientsProps().onDeliver("worker", false);
    });
    expect(useUIStore.getState().errorMsg).toBe(
      "This message was already handed to the runtime. No need to send it again (already_delivered).",
    );

    mocks.deliverMessage.mockResolvedValueOnce({
      ok: false,
      error: { code: "actor_not_found", message: "Actor missing" },
    });
    await act(async () => {
      await latestRecipientsProps().onDeliver("worker", false);
    });
    expect(useUIStore.getState().errorMsg).toBe("actor_not_found: Actor missing");
  });
});
