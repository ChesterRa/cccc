// @vitest-environment happy-dom
import { createInstance } from "i18next";
import { renderToStaticMarkup } from "react-dom/server";
import { I18nextProvider, initReactI18next } from "react-i18next";
import { beforeAll, describe, expect, it, vi } from "vite-plus/test";

import chatEn from "../../i18n/locales/en/chat.json";
import commonEn from "../../i18n/locales/en/common.json";
import { MessageBubble } from "../MessageBubble";

const i18n = createInstance();

beforeAll(async () => {
  await i18n
    .use(initReactI18next)
    .init({
      lng: "en",
      fallbackLng: "en",
      defaultNS: "chat",
      resources: { en: { chat: chatEn, common: commonEn } },
      interpolation: { escapeValue: false },
    });
});

type Outcome = { replied: boolean; cancelled: boolean };

function renderSummary(outcomes: Record<string, Outcome>, readOnly = false) {
  const host = document.createElement("div");
  host.innerHTML = renderToStaticMarkup(
    <I18nextProvider i18n={i18n}>
      <MessageBubble
        actorById={new Map()}
        actors={[]}
        displayNameMap={new Map()}
        agentState={null}
        taskById={new Map()}
        isDark={false}
        readOnly={readOnly}
        groupId="g"
        groupLabelById={{}}
        onReply={vi.fn()}
        onShowRecipients={vi.fn()}
        event={{
          id: "event-1",
          kind: "chat.message",
          by: "worker",
          data: { text: "Question", to: Object.keys(outcomes), message_mode: "request_reply" },
          _obligation_status: Object.fromEntries(
            Object.entries(outcomes).map(([id, outcome]) => [
              id,
              { reply_requested: true, delivery_state: "", ...outcome },
            ]),
          ),
        }}
      />
    </I18nextProvider>,
  );
  const label = Array.from(host.querySelectorAll("span")).find((span) =>
    /^Reply \d+\/\d+/.test(span.textContent ?? ""),
  );
  const button = host.querySelector<HTMLButtonElement>('button[aria-label^="Show reply status"]');
  const pill = label?.parentElement;
  const text = label?.textContent;
  return {
    text,
    name: button?.getAttribute("aria-label") ?? null,
    tone: pill?.getAttribute("data-obligation-state"),
  };
}

const replied = { replied: true, cancelled: false };
const cancelled = { replied: false, cancelled: true };
const open = { replied: false, cancelled: false };

describe("reply progress summary", () => {
  it("names a cancelled request by its counts instead of showing it as replied", () => {
    expect(renderSummary({ a: cancelled })).toEqual({
      text: "Reply 0/1 · 1 cancelled",
      name: "Show reply status: 0 of 1 replied, 1 cancelled",
      tone: "closed",
    });
  });

  it("keeps replies and cancellations apart when both closed a request", () => {
    expect(renderSummary({ a: replied, b: cancelled })).toEqual({
      text: "Reply 1/2 · 1 cancelled",
      name: "Show reply status: 1 of 2 replied, 1 cancelled",
      tone: "closed",
    });
  });

  it("stays open while any recipient still owes a reply", () => {
    expect(renderSummary({ a: replied, b: cancelled, c: open })).toEqual({
      text: "Reply 1/3 · 1 cancelled",
      name: "Show reply status: 1 of 3 replied, 1 cancelled",
      tone: "open",
    });
  });

  it("shows success only when every recipient replied", () => {
    expect(renderSummary({ a: replied })).toEqual({
      text: "Reply 1/1",
      name: "Show reply status: 1 of 1 replied",
      tone: "replied",
    });
  });

  it("reads the same counts as plain text in a read-only view", () => {
    expect(renderSummary({ a: cancelled }, true)).toEqual({
      text: "Reply 0/1 · 1 cancelled",
      name: null,
      tone: "closed",
    });
  });
});

describe("undelivered message", () => {
  function renderDelivery(
    states: Record<string, { state: string; reason?: string }>,
    options: { readOnly?: boolean; mode?: string } = {},
  ) {
    const host = document.createElement("div");
    host.innerHTML = renderToStaticMarkup(
      <I18nextProvider i18n={i18n}>
        <MessageBubble
          actorById={new Map()}
          actors={[]}
          displayNameMap={new Map([["peer1", "Reviewer"]])}
          agentState={null}
          taskById={new Map()}
          isDark={false}
          readOnly={options.readOnly}
          groupId="g"
          groupLabelById={{}}
          onReply={vi.fn()}
          onShowRecipients={vi.fn()}
          event={{
            id: "event-1",
            kind: "chat.message",
            by: "user",
            data: { text: "Hello", to: Object.keys(states), message_mode: options.mode ?? "send" },
            _obligation_status: Object.fromEntries(
              Object.entries(states).map(([id, { state, reason }]) => [
                id,
                {
                  replied: false,
                  reply_requested: false,
                  cancelled: false,
                  delivery_state: state,
                  delivery_reason: reason,
                },
              ]),
            ),
          }}
        />
      </I18nextProvider>,
    );
    return Array.from(host.querySelectorAll<HTMLElement>('[data-status-tone="error"]'));
  }

  it("names who did not get it, keeps the reason to hand and opens the recipients", () => {
    const [label, ...rest] = renderDelivery({
      peer1: { state: "failed", reason: "The agent stopped right after starting" },
      peer2: { state: "accepted" },
    });

    expect(rest).toEqual([]);
    expect(label.textContent).toBe("Delivery failed for Reviewer");
    expect(label.tagName).toBe("BUTTON");
    expect(label.title).toBe("The agent stopped right after starting");
  });

  it("stays quiet about delivered, sending and uncertain work", () => {
    expect(
      renderDelivery({
        a: { state: "accepted" },
        b: { state: "claimed" },
        c: { state: "ambiguous" },
      }),
    ).toEqual([]);
  });

  it("leaves Mail alone: it stays in the Inbox", () => {
    expect(renderDelivery({ peer1: { state: "failed" } }, { mode: "mail" })).toEqual([]);
  });

  it("offers no action in a read-only view", () => {
    const [label] = renderDelivery({ peer1: { state: "failed" } }, { readOnly: true });

    expect(label.tagName).toBe("SPAN");
    expect(label.textContent).toBe("Delivery failed for Reviewer");
  });
});
