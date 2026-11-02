// @vitest-environment happy-dom
import { act } from "react";
import { createRoot } from "react-dom/client";
import { renderToStaticMarkup } from "react-dom/server";
import { describe, expect, it, vi } from "vite-plus/test";
import { useUIStore } from "../../stores";
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

  it.each([
    { by: "worker", to: ["user"], source: false, expected: false },
    { by: "worker", to: ["user", "other"], source: false, expected: true },
    { by: "user", to: ["worker"], source: false, expected: true },
    { by: "worker", to: ["user"], source: true, expected: true },
  ])("keeps necessary routing labels: %j", ({ by, to, source, expected }) => {
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
    expect(host.textContent?.includes("→")).toBe(expected);
  });

  it("keeps Reply and Forward on the bar, the rest in its menu, and all in the touch menu", async () => {
    const host = document.createElement("div");
    document.body.append(host);
    const root = createRoot(host);
    const event = { id: "event-1", kind: "chat.message", by: "user", data: {} };
    const copy = vi.fn(),
      link = vi.fn(),
      context = vi.fn(),
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
            onViewContext={context}
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
      expect(bar.map((button) => button.textContent || button.getAttribute("aria-label"))).toEqual([
        "reply",
        "relay",
        "copyText",
        "messageActions",
      ]);
      // Copy text is an icon on the bar; its tooltip carries the name.
      expect(bar[2].textContent).toBe("");
      expect(bar[2].getAttribute("title")).toBe("copyText");
      await act(async () => bar[0].click());
      await act(async () => bar[1].click());
      await act(async () => bar[2].click());
      expect(reply).toHaveBeenCalledOnce();
      expect(relay).toHaveBeenCalledWith(event);
      expect(copy).toHaveBeenCalledOnce();

      const more = bar[3];
      await act(async () => more.click());
      expect(menuItems().map((item) => item.textContent)).toEqual([
        "copyLink",
        "viewMessageContext",
      ]);
      await act(async () => more.click());
      await settle();
      await choose(more, "copyLink");
      await choose(more, "viewMessageContext");
      expect(link).toHaveBeenCalledWith("event-1");
      expect(context).toHaveBeenCalledWith("event-1");

      const touch = host.querySelector<HTMLButtonElement>(".message-actions-menu button")!;
      await act(async () => touch.click());
      expect(menuItems().map((item) => item.textContent)).toEqual([
        "reply",
        "relay",
        "copyText",
        "copyLink",
        "viewMessageContext",
      ]);
      await act(async () => touch.click());
      await settle();
      await choose(touch, "reply");
      expect(reply).toHaveBeenCalledTimes(2);

      // Without a copy or a link the bar holds Reply alone: no copy key, and nothing for ⋯.
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

  it("confirms a copied message with a notice, since its key has no text to change", async () => {
    (globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;
    const writeText = vi.fn().mockResolvedValue(undefined);
    const clipboard = Object.getOwnPropertyDescriptor(navigator, "clipboard");
    Object.defineProperty(navigator, "clipboard", { configurable: true, value: { writeText } });
    const host = document.createElement("div");
    document.body.append(host);
    const root = createRoot(host);
    try {
      await act(async () =>
        root.render(
          <MessageBubble
            event={{
              id: "event-1",
              kind: "chat.message",
              by: "worker",
              data: { text: "hello", quote_text: "earlier point", insight: "provisional view" },
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
        ),
      );
      const copy = host.querySelector<HTMLButtonElement>(
        '[data-message-actions-inline] button[aria-label="copyText"]',
      )!;
      await act(async () => copy.click());
      // The quote and the sender's insight travel with the text.
      expect(writeText).toHaveBeenCalledWith(
        "> earlier point\n\nhello\n\nsenderPerspective:\nprovisional view",
      );
      expect(useUIStore.getState().notice?.message).toBe("textCopied");
    } finally {
      await act(async () => root.unmount());
      host.remove();
      if (clipboard) Object.defineProperty(navigator, "clipboard", clipboard);
      else Reflect.deleteProperty(navigator, "clipboard");
      useUIStore.getState().dismissNotice();
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
