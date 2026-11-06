// @vitest-environment happy-dom
import { act } from "react";
import { createRoot } from "react-dom/client";
import { expect, it, vi } from "vite-plus/test";
import { InboxModal } from "./InboxModal";
import type { Actor } from "../../types";

vi.mock("react-i18next", () => ({
  useTranslation: () => ({
    t: (key: string, values?: { actorId?: string }) =>
      key === "inbox.title" ? `Mail · ${values?.actorId}` : key,
    i18n: { language: "en" },
  }),
}));

it("labels the inbox once and enables mark-read only when messages are available and idle", async () => {
  Object.assign(globalThis, { IS_REACT_ACT_ENVIRONMENT: true });
  const host = document.createElement("div");
  document.body.append(host);
  const root = createRoot(host);
  const onMarkAllRead = vi.fn();
  const message = {
    id: "m1",
    kind: "chat.message",
    by: "user",
    ts: "2026-01-01T00:00:00Z",
    data: { text: "Hello" },
  };
  const render = async (messages: (typeof message)[], busy = "") => {
    await act(async () =>
      root.render(
        <InboxModal
          isOpen
          isDark={false}
          actorId="reviewer"
          actors={[]}
          messages={messages}
          busy={busy}
          onClose={() => {}}
          onMarkAllRead={onMarkAllRead}
        />,
      ),
    );
  };
  try {
    await render([]);
    const dialog = host.querySelector<HTMLElement>('[role="dialog"]')!;
    const labelId = dialog.getAttribute("aria-labelledby");
    expect(labelId).toBeTruthy();
    expect(dialog.querySelectorAll(`#${labelId}`)).toHaveLength(1);
    expect(dialog.querySelector(`#${labelId}`)?.textContent).toContain("Mail · reviewer");
    const markRead = () => host.querySelector<HTMLButtonElement>("[data-inbox-action]")!;
    expect(markRead().disabled).toBe(true);

    await render([message]);
    expect(markRead().disabled).toBe(false);
    await act(async () => markRead().click());
    expect(onMarkAllRead).toHaveBeenCalledTimes(1);

    await render([message], "inbox-mark-read");
    expect(markRead().disabled).toBe(true);
  } finally {
    await act(async () => root.unmount());
    host.remove();
  }
});

it("names senders from the actor list and keeps its fallback for user, unknown, special and empty ids", async () => {
  Object.assign(globalThis, { IS_REACT_ACT_ENVIRONMENT: true });
  const host = document.createElement("div");
  document.body.append(host);
  const root = createRoot(host);
  const senders = ["worker", "bare", "user", "ghost", "svc:chet", "@all", ""];
  const messages = senders.map((by, idx) => ({
    id: `m${idx}`,
    kind: "chat.message",
    by,
    ts: "2026-01-01T00:00:00Z",
    data: { text: "Hello" },
  }));
  try {
    await act(async () =>
      root.render(
        <InboxModal
          isOpen
          isDark={false}
          actorId="reviewer"
          actors={[{ id: "worker", title: "Review lead" } as Actor, { id: "bare" } as Actor]}
          messages={messages}
          busy=""
          onClose={() => {}}
          onMarkAllRead={() => {}}
        />,
      ),
    );
    const labels = [...host.querySelectorAll("[data-inbox-sender]")].map((el) => el.textContent);
    expect(labels).toEqual(["Review lead", "bare", "user", "ghost", "svc:chet", "@all", "—"]);
  } finally {
    await act(async () => root.unmount());
    host.remove();
  }
});
