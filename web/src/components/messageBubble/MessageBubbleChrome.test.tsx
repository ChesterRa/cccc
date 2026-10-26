// @vitest-environment happy-dom
import { act } from "react";
import { createRoot } from "react-dom/client";
import { renderToStaticMarkup } from "react-dom/server";
import { describe, expect, it, vi } from "vite-plus/test";
import { MessageBubble } from "../MessageBubble";
import { MessageActions } from "./MessageActions";

vi.mock("react-i18next", () => ({ useTranslation: () => ({ t: (key: string) => key }) }));

describe("message chrome", () => {
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
    if (by === "user") {
      const names = Array.from(host.querySelectorAll("span")).filter(
        (n) => n.textContent === "user" && n.classList.contains("sr-only"),
      );
      expect(names.length).toBe(2);
    }
  });

  it("binds inline and menu actions to the exact message and closes the menu after reply", async () => {
    const host = document.createElement("div");
    document.body.append(host);
    const root = createRoot(host);
    const event = { id: "event-1", kind: "chat.message", by: "user", data: {} };
    const copy = vi.fn(),
      link = vi.fn(),
      context = vi.fn(),
      relay = vi.fn(),
      reply = vi.fn();
    try {
      await act(async () =>
        root.render(
          <MessageActions
            copiedMessageText={false}
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
      for (const button of host.querySelectorAll<HTMLButtonElement>(
        "[data-message-actions-inline] button",
      ))
        await act(async () => button.click());
      expect(copy).toHaveBeenCalledOnce();
      expect(context).toHaveBeenCalledWith("event-1");
      expect(link).toHaveBeenCalledWith("event-1");
      expect(relay).toHaveBeenCalledWith(event);
      expect(reply).toHaveBeenCalledOnce();
      const trigger = host.querySelector<HTMLButtonElement>('button[aria-label="messageActions"]')!;
      await act(async () => trigger.click());
      expect(trigger.getAttribute("aria-expanded")).toBe("true");
      const menuReply = Array.from(
        document.querySelectorAll<HTMLButtonElement>('[role="dialog"] button'),
      ).find((button) => button.textContent === "reply")!;
      await act(async () => menuReply.click());
      expect(reply).toHaveBeenCalledTimes(2);
      expect(trigger.getAttribute("aria-expanded")).toBe("false");
      await act(async () =>
        root.render(
          <MessageActions
            readOnly
            copiedMessageText={false}
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
});
