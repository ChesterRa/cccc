// @vitest-environment happy-dom
import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, expect, it, vi } from "vite-plus/test";
import { MessageBubble } from "./MessageBubble";
import {
  LivePendingStatusContext,
  type LivePendingStatus,
} from "../features/pendingReplies/liveStatusContext";
import {
  PendingReplyStoreContext,
  createPendingReplyStore,
  type PendingReplyStoreApi,
} from "../features/pendingReplies/store";
import { useGroupStore } from "../stores";
import { updateObligationAtIndex } from "../stores/groupStoreCore";
import type { GroupDoc, LedgerEvent } from "../types";

vi.mock("react-i18next", () => ({ useTranslation: () => ({ t: (key: string) => key }) }));

Object.assign(globalThis, { IS_REACT_ACT_ENVIRONMENT: true });

const group: GroupDoc = { group_id: "g1", generation: "generation-1" };
const fromChet = (marker: string): LedgerEvent => ({
  id: "e1",
  kind: "chat.message",
  by: "svc:chet",
  ts: "2026-11-02T08:00:00Z",
  data: { text: "Please check the build.", to: ["user"], _cccc_chet_group_generation: marker },
});

let host: HTMLDivElement;
let root: Root;
let pendingStore: PendingReplyStoreApi;

beforeEach(() => {
  host = document.createElement("div");
  document.body.appendChild(host);
  root = createRoot(host);
  pendingStore = createPendingReplyStore();
  useGroupStore.setState({ groupDoc: group });
});

afterEach(() => {
  act(() => root.unmount());
  host.remove();
  useGroupStore.setState({ groupDoc: null });
});

async function render(event: LedgerEvent, status?: LivePendingStatus) {
  await act(async () =>
    root.render(
      <PendingReplyStoreContext.Provider value={pendingStore}>
        <LivePendingStatusContext.Provider
          value={status && event.id ? { [event.id]: status } : null}
        >
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
          />
        </LivePendingStatusContext.Provider>
      </PendingReplyStoreContext.Provider>,
    ),
  );
}

it("names Chet only on a message this Group's generation proves Chet wrote", async () => {
  await render(fromChet("generation-1"));
  expect(host.querySelector(".lcd-row-name")?.textContent).toBe("Chet");

  // A copied or re-created Group keeps the message but not the proof.
  await render(fromChet("generation-0"));
  expect(host.querySelector(".lcd-row-name")?.textContent).toBe("svc:chet");
});

it("shows the stored identity until the Group is loaded", async () => {
  useGroupStore.setState({ groupDoc: null });
  await render(fromChet("generation-1"));
  expect(host.querySelector(".lcd-row-name")?.textContent).toBe("svc:chet");
});

it("shows the Chet author and closes the original card before the next status read", async () => {
  const request: LedgerEvent = {
    id: "request",
    kind: "chat.message",
    by: "peer",
    data: { message_mode: "request_reply", to: ["user"], text: "Use what directory?" },
    _user_request: { question: "none", requester: "available" },
    _obligation_status: {
      user: { reply_requested: true, replied: false, cancelled: false, delivery_state: "" },
    },
  };
  const lastStatus = {
    status: { user_request: request._user_request, obligation_status: request._obligation_status },
    checking: false,
  };
  await render(request, lastStatus);
  expect(host.querySelector("[data-pending-reply]")).not.toBeNull();

  const ordinary = fromChet("generation-1");
  const reply: LedgerEvent = {
    ...ordinary,
    data: { ...ordinary.data, reply_to: request.id, to: ["peer"], _cccc_chet_on_behalf_of: "user" },
  };
  const { next } = updateObligationAtIndex([request], 0, { reply });
  await render(next[0], lastStatus);
  expect(host.querySelector("[data-pending-reply]")).toBeNull();
  expect(host.querySelector("[data-pending-reply-closed]")?.textContent).toContain(
    "pendingReply.chetAnswered",
  );

  await render(reply);
  expect(host.querySelector(".lcd-row-name")?.textContent).toBe("pendingReply.chetAnswered");
});
