// @vitest-environment happy-dom
import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, it, vi } from "vite-plus/test";
import { useUIStore } from "../../stores";
import type { Actor, LedgerEvent } from "../../types";
import { MessageBubble } from "../MessageBubble";
import { MessageActions } from "./MessageActions";

const replyMessage = vi.hoisted(() => vi.fn());
vi.mock("../../services/api", async (importOriginal) => ({
  ...(await importOriginal<typeof import("../../services/api")>()),
  replyMessage,
}));
vi.mock("react-i18next", () => ({ useTranslation: () => ({ t: (key: string) => key }) }));

const QUICK = ["quickReply.proceed", "quickReply.why", "quickReply.rethink", "quickReply.choices"];
const worker = { id: "worker", title: "Worker", runtime: "codex", enabled: true } as Actor;
const agentMessage: LedgerEvent = {
  id: "event-1",
  kind: "chat.message",
  by: "worker",
  group_id: "g",
  ts: "2026-11-03T00:00:00Z",
  data: { text: "Plan ready", to: ["user"] },
};

let host: HTMLDivElement;
let root: Root;
const menuItems = () =>
  Array.from(document.querySelectorAll<HTMLButtonElement>('[role="dialog"] button'));
const menuGroups = () =>
  Array.from(document.querySelectorAll('[role="dialog"] [role="group"]')).map((group) =>
    group.getAttribute("aria-label"),
  );

async function renderBubble(event: LedgerEvent, extra: { readOnly?: boolean } = {}) {
  await act(async () =>
    root.render(
      <MessageBubble
        event={event}
        actorById={new Map([["worker", worker]])}
        actors={[worker]}
        displayNameMap={new Map([["worker", "Worker"]])}
        agentState={null}
        taskById={new Map()}
        isDark={false}
        readOnly={extra.readOnly}
        groupId="g"
        groupLabelById={{}}
        onReply={vi.fn()}
        onShowRecipients={vi.fn()}
      />,
    ),
  );
}

async function touchMenuLabels() {
  const touch = host.querySelector<HTMLButtonElement>(".message-actions-menu button");
  if (!touch) return [];
  await act(async () => touch.click());
  const labels = menuItems().map((item) => item.textContent);
  await act(async () => touch.click());
  return labels;
}

beforeEach(() => {
  Object.assign(globalThis, { IS_REACT_ACT_ENVIRONMENT: true });
  replyMessage.mockReset();
  host = document.createElement("div");
  document.body.append(host);
  root = createRoot(host);
});
afterEach(async () => {
  await act(async () => root.unmount());
  host.remove();
});

describe("quick replies", () => {
  it("keep their own labelled part of the menus, before Copy link", async () => {
    const run = vi.fn();
    const quickReplies = QUICK.map((label) => ({ key: label, label, run }));
    const event = { id: "event-1", kind: "chat.message", by: "worker", data: {} };
    await act(async () =>
      root.render(
        <MessageActions
          copyableMessageText="text"
          onCopyMessageText={vi.fn()}
          onCopyLink={vi.fn()}
          onRelay={vi.fn()}
          onReply={vi.fn()}
          canReply
          eventId={event.id}
          event={event}
          quickReplies={quickReplies}
        />,
      ),
    );
    const more = host.querySelector<HTMLButtonElement>(
      '[data-message-actions-inline] button[aria-label="messageActions"]',
    )!;
    await act(async () => more.click());
    expect(menuItems().map((item) => item.textContent)).toEqual([...QUICK, "copyLink"]);
    expect(menuGroups()).toEqual(["quickReply.menu", "messageActions"]);
    await act(async () => menuItems()[0].click());
    expect(run).toHaveBeenCalledOnce();
  });

  it("are offered on a message an agent of this Group wrote here", async () => {
    await renderBubble(agentMessage);
    expect(await touchMenuLabels()).toEqual(expect.arrayContaining(QUICK));
  });

  it.each([
    ["your own message", { ...agentMessage, by: "user" }],
    ["a member no longer in the Group", { ...agentMessage, by: "gone" }],
    ["Chet", { ...agentMessage, by: "svc:chet" }],
    ["a message still being written", { ...agentMessage, _streaming: true }],
    ["a question with its own answer card", { ...agentMessage, data: { text: "?", question: {} } }],
    // The daemon writes a message from another Group under that Group's ID and the sender's.
    [
      "a message from another Group",
      { ...agentMessage, by: "g2::worker", data: { text: "hi", src_group_id: "g2" } },
    ],
    // A plain send may also carry relay provenance, under the member's own ID.
    [
      "a member's relay from another Group",
      {
        ...agentMessage,
        data: { text: "hi", to: ["user"], src_group_id: "g2", src_event_id: "e9" },
      },
    ],
    // Member IDs often repeat across Groups, so the recipient there can share the author's ID.
    [
      "a message sent to another Group",
      {
        ...agentMessage,
        data: { text: "hi", to: ["user"], dst_group_id: "g2", dst_to: ["worker"] },
      },
    ],
    [
      "a message sent to another instance",
      { ...agentMessage, data: { text: "hi", dst_instance_id: "i2", dst_group_id: "g2" } },
    ],
    [
      "an IM message",
      { ...agentMessage, by: "user", data: { text: "hi", source_platform: "telegram" } },
    ],
  ])("are not offered on %s", async (_name, event) => {
    await renderBubble(event as LedgerEvent);
    const labels = await touchMenuLabels();
    for (const label of QUICK) expect(labels).not.toContain(label);
  });

  it("are not offered in a read-only view", async () => {
    await renderBubble(agentMessage, { readOnly: true });
    expect(host.querySelector(".message-actions-menu button")).toBeNull();
  });

  it("send the ready text to the author as a reply and say so", async () => {
    const showNotice = vi.fn();
    useUIStore.setState({ showNotice });
    replyMessage.mockResolvedValue({ ok: true, result: { event: { id: "reply-1" } } });
    await renderBubble(agentMessage);
    const touch = host.querySelector<HTMLButtonElement>(".message-actions-menu button")!;
    await act(async () => touch.click());
    await act(async () =>
      menuItems()
        .find((item) => item.textContent === "quickReply.proceed")!
        .click(),
    );
    expect(replyMessage).toHaveBeenCalledWith(
      "g",
      "quickReply.proceedText",
      ["worker"],
      "event-1",
      undefined,
      expect.any(String),
      [],
      "Plan ready",
      "send",
    );
    expect(showNotice).toHaveBeenCalledWith({ message: "quickReply.sent" });
  });

  it("offers to retry a reply whose result is unknown", async () => {
    const showNotice = vi.fn();
    useUIStore.setState({ showNotice });
    replyMessage.mockResolvedValue({
      ok: false,
      error: { code: "NETWORK_ERROR", message: "Network request failed" },
    });
    await renderBubble(agentMessage);
    const touch = host.querySelector<HTMLButtonElement>(".message-actions-menu button")!;
    await act(async () => touch.click());
    await act(async () =>
      menuItems()
        .find((item) => item.textContent === "quickReply.why")!
        .click(),
    );
    const notice = showNotice.mock.calls[0][0];
    expect(notice).toMatchObject({
      message: "quickReply.unknown",
      actionLabel: "quickReply.retry",
    });
    // The retry still works once the row is gone, as a virtual list unmounts rows.
    await act(async () => root.render(<div />));
    replyMessage.mockResolvedValue({ ok: true, result: { event: { id: "reply-1" } } });
    await act(async () => notice.onAction());
    expect(replyMessage).toHaveBeenCalledTimes(2);
    expect(replyMessage.mock.calls[1][5]).toBe(replyMessage.mock.calls[0][5]);
    expect(showNotice).toHaveBeenLastCalledWith({ message: "quickReply.sent" });
  });
});
