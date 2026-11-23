// @vitest-environment happy-dom
import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, it, vi } from "vite-plus/test";
import type { Actor, LedgerEvent } from "../../types";
import { MessageBubble } from "../MessageBubble";

vi.mock("react-i18next", () => ({ useTranslation: () => ({ t: (key: string) => key }) }));

const worker = { id: "worker", title: "Worker", runtime: "codex", enabled: true } as Actor;

function sent(status: Record<string, Record<string, unknown>>, mode = "send"): LedgerEvent {
  return {
    id: "event-1",
    kind: "chat.message",
    by: "user",
    group_id: "g",
    ts: "2026-11-22T00:00:00Z",
    data: { text: "Please look", to: Object.keys(status), message_mode: mode },
    _obligation_status: Object.fromEntries(
      Object.entries(status).map(([id, fields]) => [
        id,
        { replied: false, reply_requested: false, cancelled: false, ...fields },
      ]),
    ),
  } as LedgerEvent;
}

let host: HTMLDivElement;
let root: Root;
let onShowRecipients: ReturnType<typeof vi.fn<() => void>>;

async function render(event: LedgerEvent, readOnly = false) {
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
        readOnly={readOnly}
        groupId="g"
        groupLabelById={{}}
        onReply={vi.fn()}
        onShowRecipients={onShowRecipients}
      />,
    ),
  );
}

async function openTouchMenu() {
  const touch = host.querySelector<HTMLButtonElement>(".message-actions-menu button");
  if (!touch) return [];
  await act(async () => touch.click());
  return Array.from(document.querySelectorAll<HTMLButtonElement>('[role="dialog"] button'));
}

beforeEach(() => {
  Object.assign(globalThis, { IS_REACT_ACT_ENVIRONMENT: true });
  onShowRecipients = vi.fn<() => void>();
  host = document.createElement("div");
  document.body.append(host);
  root = createRoot(host);
});
afterEach(async () => {
  await act(async () => root.unmount());
  host.remove();
});

describe("recipient status from the action menu", () => {
  // A plain Send's footer shows nothing for delivered or waiting recipients, so the menu is the
  // way to see where it went or why it waits, and to send it now.
  it.each([
    ["waiting", { delivery_state: "failed", waiting_reason: "group_paused" }],
    [
      "written to the terminal",
      { delivery_state: "accepted", delivery_evidence: "terminal_written" },
    ],
  ])("opens the recipient list for a plain Send that is %s", async (_, fields) => {
    await render(sent({ worker: fields }));
    const item = (await openTouchMenu()).find((b) => b.textContent === "showRecipientStatus");
    expect(item).toBeDefined();
    await act(async () => item!.click());
    expect(onShowRecipients).toHaveBeenCalledOnce();
    // Still no failure mark on the bubble for the waiting one.
    expect(host.querySelector('[data-status-tone="error"]')).toBeNull();
  });

  it("is left out where the footer already opens it: a reply count", async () => {
    await render(
      sent({ worker: { delivery_state: "accepted", reply_requested: true } }, "request_reply"),
    );
    const labels = (await openTouchMenu()).map((b) => b.textContent);
    expect(labels).not.toContain("showRecipientStatus");
  });

  it("is left out without a recipient status of a member", async () => {
    await render(sent({ user: { delivery_state: "accepted" } }));
    const labels = (await openTouchMenu()).map((b) => b.textContent);
    expect(labels).not.toContain("showRecipientStatus");
  });

  it("is not offered in a read-only view", async () => {
    await render(
      sent({ worker: { delivery_state: "failed", waiting_reason: "not_submitted" } }),
      true,
    );
    expect(host.querySelector(".message-actions-menu button")).toBeNull();
  });
});
