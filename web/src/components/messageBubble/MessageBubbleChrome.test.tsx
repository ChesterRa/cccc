// @vitest-environment happy-dom
import { act } from "react";
import { createRoot } from "react-dom/client";
import { renderToStaticMarkup } from "react-dom/server";
import { describe, expect, it, vi } from "vite-plus/test";
import type { LedgerEvent } from "../../types";
import { MessageBubble } from "../MessageBubble";
import { MessageActions } from "./MessageActions";

vi.mock("react-i18next", () => ({ useTranslation: () => ({ t: (key: string) => key }) }));

describe("message chrome", () => {
  it.each(["submitting", "submitted", "pending", "bound", "ambiguous", "failed"] as const)(
    "marks delivery %s as moving only during submission",
    (state) => {
      const markup = renderToStaticMarkup(
        <MessageBubble
          event={{ id: "e", kind: "chat.message", by: "user", data: { text: "hello" } }}
          actorById={new Map()}
          actors={[]}
          displayNameMap={new Map()}
          agentState={null}
          taskById={new Map()}
          isDark={false}
          readOnly
          groupId="g"
          groupLabelById={{}}
          onReply={vi.fn()}
          onShowRecipients={vi.fn()}
          webModelDeliveryStatus={{
            state,
            actorId: "worker",
            deliveryId: "delivery",
            updatedAt: "",
            detail: "",
          }}
        />,
      );
      expect(markup.includes('data-status-motion="working"')).toBe(state === "submitting");
    },
  );

  it("keeps attachments with the message's text, before the sender's insight", () => {
    const host = document.createElement("div");
    host.innerHTML = renderToStaticMarkup(
      <MessageBubble
        event={{
          id: "e",
          kind: "chat.message",
          by: "worker",
          data: {
            text: "Here is the settings page.",
            to: ["user"],
            insight: "The event subscription is still missing.",
            attachments: [
              {
                kind: "image",
                path: "state/blobs/a_page.png",
                title: "page.png",
                mime_type: "image/png",
              },
            ],
          },
        }}
        actorById={new Map()}
        actors={[]}
        displayNameMap={new Map()}
        agentState={null}
        taskById={new Map()}
        isDark={false}
        groupId="g"
        groupLabelById={{}}
        onReply={vi.fn()}
        onShowRecipients={vi.fn()}
      />,
    );
    const text = [...host.querySelectorAll("p, div")].find(
      (node) => node.textContent === "Here is the settings page.",
    );
    const attachments = host.querySelector("[data-message-attachments]");
    const insight = host.querySelector("[data-message-insight]");
    const follows = (a: Node, b: Node) =>
      Boolean(a.compareDocumentPosition(b) & Node.DOCUMENT_POSITION_FOLLOWING);
    expect(text && attachments && insight).toBeTruthy();
    expect(follows(text!, attachments!)).toBe(true);
    expect(follows(attachments!, insight!)).toBe(true);
    // No rule parts the image from the text it was sent with.
    expect(attachments!.className).not.toMatch(/border-t|pt-3/);
  });

  it("shows one reply-request label and preserves recipient progress", () => {
    const props = {
      actorById: new Map(),
      actors: [],
      displayNameMap: new Map(),
      agentState: null,
      taskById: new Map(),
      isDark: false,
      groupId: "g",
      groupLabelById: {},
      onReply: vi.fn(),
      onShowRecipients: vi.fn(),
    };
    const host = document.createElement("div");
    const message = {
      id: "event-1",
      kind: "chat.message",
      by: "worker",
      data: { text: "Question", to: ["user"], message_mode: "request_reply" },
    };
    host.innerHTML = renderToStaticMarkup(<MessageBubble {...props} event={message} />);
    expect(host.textContent?.match(/needReply/g)).toHaveLength(1);

    host.innerHTML = renderToStaticMarkup(
      <MessageBubble
        {...props}
        event={{
          ...message,
          data: { ...message.data, to: ["other"] },
          _obligation_status: {
            other: { reply_requested: true, replied: false, cancelled: false, delivery_state: "" },
          },
        }}
      />,
    );
    expect(host.textContent?.match(/needReply/g)).toHaveLength(1);
    expect(host.textContent).toContain("reply 0/1");
  });

  it("tells a cancelled request apart from a replied one, visibly and by name", () => {
    const props = {
      actorById: new Map(),
      actors: [],
      displayNameMap: new Map(),
      agentState: null,
      taskById: new Map(),
      isDark: false,
      groupId: "g",
      groupLabelById: {},
      onReply: vi.fn(),
      onShowRecipients: vi.fn(),
    };
    const request = (other: { replied: boolean; cancelled: boolean }) => ({
      id: "event-1",
      kind: "chat.message",
      by: "worker",
      data: { text: "Question", to: ["other"], message_mode: "request_reply" },
      _obligation_status: { other: { reply_requested: true, delivery_state: "", ...other } },
    });
    const host = document.createElement("div");

    host.innerHTML = renderToStaticMarkup(
      <MessageBubble {...props} event={request({ replied: false, cancelled: true })} />,
    );
    const cancelled = host.querySelector('button[aria-label="showObligationStatusCancelled"]');
    expect(cancelled?.textContent).toBe("reply 0/1 · replyCancelledCount");
    expect(cancelled?.getAttribute("data-status-tone")).toBe("muted");

    host.innerHTML = renderToStaticMarkup(
      <MessageBubble {...props} event={request({ replied: true, cancelled: false })} />,
    );
    const replied = host.querySelector('button[aria-label="showObligationStatusCounts"]');
    expect(replied?.textContent).toBe("reply 1/1");
    expect(replied?.getAttribute("data-status-tone")).toBe("ok");
  });

  it("keeps Mail, recipients and failure evidence readable in a read-only grouped row", () => {
    const markup = renderToStaticMarkup(
      <MessageBubble
        event={{
          id: "event-1",
          kind: "chat.message",
          by: "user",
          data: { text: "request", to: ["worker"], message_mode: "mail" },
          _connect_delivery: { state: "failed", error: "Connection refused" },
        }}
        actorById={new Map()}
        actors={[]}
        displayNameMap={new Map([["worker", "Worker"]])}
        agentState={null}
        taskById={new Map()}
        isDark={false}
        readOnly
        collapseHeader
        groupId="g"
        groupLabelById={{}}
        onReply={vi.fn()}
        onShowRecipients={vi.fn()}
      />,
    );
    expect(markup).toContain("mailMessageHint");
    expect(markup).toContain("modeMail");
    expect(markup).toContain("Worker");
    expect(markup).toContain("Connection refused");
    expect(markup).not.toContain("messageActions");
    expect(markup).not.toContain("copyText");
  });

  // A message to the user names its recipient too, so no route is left to be guessed.
  it.each([
    { by: "worker", to: ["user"], source: false },
    { by: "worker", to: ["user", "other"], source: false },
    { by: "user", to: ["worker"], source: false },
    { by: "worker", to: ["user"], source: true },
  ])("shows every message's recipients: %j", ({ by, to, source }) => {
    const host = document.createElement("div");
    host.innerHTML = renderToStaticMarkup(
      <MessageBubble
        event={{
          id: "e",
          kind: "chat.message",
          by,
          data: {
            text: "hello",
            to,
            ...(source ? { src_group_id: "remote", src_event_id: "source" } : {}),
          },
        }}
        actorById={new Map()}
        actors={[]}
        displayNameMap={new Map()}
        agentState={null}
        taskById={new Map()}
        isDark={false}
        groupId="g"
        groupLabelById={{}}
        onReply={vi.fn()}
        onShowRecipients={vi.fn()}
      />,
    );
    expect(host.textContent?.includes("→")).toBe(true);
  });

  // A filtered view gives each message a key into the whole conversation: first on the hover
  // bar, and after the time for screens without the bar (lcd.css shows one or the other), never
  // in a menu. A message still being sent or written has no place there yet.
  it("puts a filtered message's way into the conversation on its bar and after its time", async () => {
    const host = document.createElement("div");
    document.body.append(host);
    const root = createRoot(host);
    const open = vi.fn();
    const show = (event: LedgerEvent, filtered = true) =>
      act(async () =>
        root.render(
          <MessageBubble
            event={event}
            actorById={new Map()}
            actors={[]}
            displayNameMap={new Map()}
            agentState={null}
            taskById={new Map()}
            isDark={false}
            groupId="g"
            groupLabelById={{}}
            onReply={vi.fn()}
            onShowRecipients={vi.fn()}
            onViewContext={filtered ? open : undefined}
          />,
        ),
      );
    const key = () => host.querySelector<HTMLButtonElement>(".lcd-row-jump");
    try {
      await show({ id: "event-1", kind: "chat.message", by: "worker", data: { text: "hi" } });
      expect(key()?.textContent).toBe("showInConversation");
      await act(async () => key()!.click());
      expect(open).toHaveBeenCalledWith("event-1");
      const bar = host.querySelectorAll<HTMLButtonElement>("[data-message-actions-inline] button");
      expect(bar[0]?.textContent).toBe("showInConversation");
      // lcd.css hides one of the two by the column's width, through these marks.
      expect(bar[0].hasAttribute("data-show-in-chat")).toBe(true);
      expect(host.querySelector("[data-lcd-show-in-chat]")).not.toBeNull();
      await act(async () => bar[0].click());
      expect(open).toHaveBeenCalledTimes(2);
      const touch = host.querySelector<HTMLButtonElement>(".message-actions-menu button")!;
      await act(async () => touch.click());
      expect(
        Array.from(document.querySelectorAll('[role="dialog"] button')).map(
          (item) => item.textContent,
        ),
      ).not.toContain("showInConversation");
      await act(async () => touch.click());

      await show(
        { id: "event-1", kind: "chat.message", by: "worker", data: { text: "hi" } },
        false,
      );
      expect(key()).toBeNull();
      await show({
        id: "local-1",
        kind: "chat.message",
        by: "user",
        data: { text: "hi", _optimistic: true },
      });
      expect(key()).toBeNull();
      await show({
        id: "stream-1",
        kind: "chat.message",
        by: "worker",
        _streaming: true,
        data: { text: "hi" },
      });
      expect(key()).toBeNull();
    } finally {
      await act(async () => root.unmount());
      host.remove();
    }
  });

  it("keeps Reply and Forward on the bar, the rest in its menu, and all in the touch menu", async () => {
    const host = document.createElement("div");
    document.body.append(host);
    const root = createRoot(host);
    const event = { id: "event-1", kind: "chat.message", by: "user", data: {} };
    const copy = vi.fn(),
      link = vi.fn(),
      relay = vi.fn(),
      reply = vi.fn();
    const menuItems = () =>
      Array.from(document.querySelectorAll<HTMLButtonElement>('[role="dialog"] button'));
    // A closing menu returns focus to its trigger on the next task; let that land
    // before another menu opens, as it has by a person's next click.
    const settle = () => act(() => new Promise((resolve) => setTimeout(resolve, 0)));
    const choose = async (trigger: HTMLButtonElement, label: string) => {
      await act(async () => trigger.click());
      expect(trigger.getAttribute("aria-expanded")).toBe("true");
      await act(async () =>
        menuItems()
          .find((item) => item.textContent === label)!
          .click(),
      );
      expect(trigger.getAttribute("aria-expanded")).toBe("false");
      await settle();
    };
    try {
      await act(async () =>
        root.render(
          <MessageActions
            copyableMessageText="text"
            onCopyMessageText={copy}
            onCopyLink={link}
            onRelay={relay}
            onReply={reply}
            canReply
            eventId={event.id}
            event={event}
          />,
        ),
      );
      const bar = Array.from(
        host.querySelectorAll<HTMLButtonElement>("[data-message-actions-inline] button"),
      );
      await act(async () => bar[0].click());
      await act(async () => bar[1].click());
      expect(reply).toHaveBeenCalledOnce();
      expect(relay).toHaveBeenCalledWith(event);

      const more = host.querySelector<HTMLButtonElement>(
        '[data-message-actions-inline] button[aria-label="messageActions"]',
      )!;
      await act(async () => more.click());
      expect(menuItems().map((item) => item.textContent)).toEqual(["copyLink"]);
      await act(async () => more.click());
      await settle();
      await choose(more, "copyLink");
      expect(link).toHaveBeenCalledWith("event-1");

      const touch = host.querySelector<HTMLButtonElement>(".message-actions-menu button")!;
      await act(async () => touch.click());
      await act(async () => touch.click());
      await settle();
      await choose(touch, "reply");
      expect(reply).toHaveBeenCalledTimes(2);

      // Without a link the bar holds Reply alone, with nothing for ⋯.
      await act(async () =>
        root.render(
          <MessageActions
            copyableMessageText=""
            onCopyMessageText={copy}
            onReply={reply}
            canReply
            event={{ kind: "chat.message", by: "user", data: {} }}
          />,
        ),
      );
      expect(
        Array.from(host.querySelectorAll("[data-message-actions-inline] button")).map(
          (button) => button.textContent,
        ),
      ).toEqual(["reply"]);
      await act(async () =>
        root.render(
          <MessageActions
            readOnly
            copyableMessageText="text"
            onCopyMessageText={copy}
            onReply={reply}
            canReply
            eventId={event.id}
            event={event}
          />,
        ),
      );
      expect(host.querySelector("button")).toBeNull();
    } finally {
      await act(async () => root.unmount());
      host.remove();
    }
  });

  it("removes message actions when an existing row becomes read-only", async () => {
    (globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;
    const host = document.createElement("div");
    document.body.append(host);
    const root = createRoot(host);
    const props = {
      event: { id: "event-1", kind: "chat.message", by: "worker", data: { text: "hello" } },
      actorById: new Map(),
      actors: [],
      displayNameMap: new Map(),
      agentState: null,
      taskById: new Map(),
      isDark: false,
      groupId: "g",
      groupLabelById: {},
      onReply: vi.fn(),
      onShowRecipients: vi.fn(),
    };
    try {
      await act(async () => root.render(<MessageBubble {...props} readOnly={false} />));
      expect(host.querySelector('[aria-label="messageActions"]')).not.toBeNull();
      await act(async () => root.render(<MessageBubble {...props} readOnly />));
      expect(host.querySelector('[aria-label="messageActions"]')).toBeNull();
      await act(async () => root.render(<MessageBubble {...props} readOnly={false} />));
      expect(host.querySelector('[aria-label="messageActions"]')).not.toBeNull();
    } finally {
      await act(async () => root.unmount());
      host.remove();
    }
  });
});
