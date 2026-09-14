import { renderToStaticMarkup } from "react-dom/server";
import { describe, expect, it, vi } from "vite-plus/test";
import { MessageBubble } from "./MessageBubble";
import type { LedgerEvent } from "../types";

vi.mock("react-i18next", () => ({
  useTranslation: () => ({
    t: (key: string, values?: Record<string, string>) =>
      values?.label ? `${key}: ${values.label}` : key,
  }),
}));

function render(event: LedgerEvent) {
  return renderToStaticMarkup(
    <MessageBubble
      event={event}
      actorById={new Map()}
      actors={[]}
      displayNameMap={new Map([["worker", "Unrelated local worker"]])}
      agentState={null}
      taskById={new Map()}
      isDark={false}
      groupId="g_local"
      groupLabelById={{ g_collision: "Unrelated local Group" }}
      onReply={() => undefined}
      onShowRecipients={() => undefined}
      onOpenSource={() => undefined}
    />,
  );
}

describe("Connect message presentation", () => {
  it("shows the received Group and Actor snapshots without a local source jump", () => {
    const markup = render({
      id: "received",
      kind: "chat.message",
      by: "connect:instance-b12345678",
      data: {
        text: "Remote answer",
        message_mode: "send",
        to: ["user"],
        source_user_name: "Remote worker",
        src_instance_id: "instance-b12345678",
        src_instance_name: "Office computer",
        src_group_id: "g_collision",
        src_group_title: "Remote Group",
        src_event_id: "remote-event",
      },
    });
    expect(markup).toContain("Remote worker");
    expect(markup).toContain("Office computer / Remote Group");
    expect(markup).toContain("instance-b12345678 / g_collision");
    expect(markup).not.toContain("Unrelated local Group");
    expect(markup).not.toMatch(/<button[^>]*>[^]*?relayedFrom[^]*?<\/button>/);
  });

  it("shows remote recipients and failure evidence on the outgoing message", () => {
    const markup = render({
      id: "outgoing",
      kind: "chat.message",
      by: "user",
      _connect_delivery: { state: "unconfirmed", error: "The target did not confirm delivery." },
      data: {
        text: "Remote question",
        message_mode: "send",
        to: ["user"],
        dst_group_id: "g_collision",
        dst_group_title: "Remote Group",
        dst_instance_id: "instance-b12345678",
        dst_instance_name: "Office computer",
        dst_to: ["worker"],
        dst_actor_titles: { worker: "Remote worker" },
      },
    });
    expect(markup).toContain("Remote worker");
    expect(markup).toContain("Office computer / Remote Group");
    expect(markup).toContain("connectDelivery.unconfirmed");
    expect(markup).toContain("The target did not confirm delivery.");
    expect(markup).not.toContain("Unrelated local");
  });
});

it("uses the actual remote Actor ID for old messages with no custom title or instance name", () => {
  const markup = render({
    id: "old-reply",
    kind: "chat.message",
    by: "connect:instance-b12345678",
    data: {
      text: "Handshake reply",
      to: ["user"],
      source_user_name: "",
      source_user_id: "worker",
      src_instance_id: "instance-b12345678",
      src_group_id: "g_collision",
      src_group_title: "Remote Group",
      src_event_id: "original",
    },
  });
  expect(markup).toContain("worker");
  expect(markup).not.toContain("remoteGroupFallback");
  expect(markup).not.toContain("Unrelated local worker");
  expect(markup).not.toContain("Remote Group · 12345678");
  expect(markup).toContain("instance-b12345678 / g_collision");
});

it("shows message delivery and cancellation failure separately", () => {
  const markup = render({
    id: "source",
    kind: "chat.message",
    by: "user",
    data: {
      text: "question",
      dst_instance_id: "remote",
      dst_group_id: "target",
      dst_to: ["worker"],
    },
    _connect_delivery: { state: "sent" },
    _connect_cancellation: { state: "failed", error: "The target did not accept cancellation." },
  });
  expect(markup).toContain("connectDelivery.sent");
  expect(markup).toContain("connectCancellation.failed");
  expect(markup).toContain("The target did not accept cancellation.");
});
