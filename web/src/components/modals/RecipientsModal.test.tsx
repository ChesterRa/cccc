import { createInstance } from "i18next";
import { renderToStaticMarkup } from "react-dom/server";
import { I18nextProvider, initReactI18next } from "react-i18next";
import { beforeAll, describe, expect, it } from "vite-plus/test";

import chatEn from "../../i18n/locales/en/chat.json";
import commonEn from "../../i18n/locales/en/common.json";
import modalsEn from "../../i18n/locales/en/modals.json";
import { RecipientsModal, type RecipientEntry, type RecipientsModalProps } from "./RecipientsModal";

const i18n = createInstance();

beforeAll(async () => {
  await i18n
    .use(initReactI18next)
    .init({
      lng: "en",
      fallbackLng: "en",
      defaultNS: "modals",
      resources: { en: { common: commonEn, modals: modalsEn, chat: chatEn } },
      interpolation: { escapeValue: false },
    });
});

function entry(overrides: Partial<RecipientEntry> = {}): RecipientEntry {
  return {
    id: "peer1",
    cleared: false,
    deliveryState: "",
    read: false,
    replied: false,
    replyRequested: false,
    cancelled: false,
    ...overrides,
  };
}

function renderModal(entries: RecipientEntry[], overrides: Partial<RecipientsModalProps> = {}) {
  return renderToStaticMarkup(
    <I18nextProvider i18n={i18n}>
      <RecipientsModal
        isOpen
        isDark={false}
        toLabel="peer1"
        statusKind="read"
        entries={entries}
        messageMode="mail"
        busyAction=""
        canCancelReply={false}
        onDeliver={() => undefined}
        onCancelReply={() => undefined}
        onClose={() => undefined}
        {...overrides}
      />
    </I18nextProvider>,
  );
}

/** Each recipient's status cell: [role, accessible name, visible text, shown as success]. */
function statusCells(markup: string) {
  return Array.from(
    markup.matchAll(
      /<div class="([^"]*tracking-tight[^"]*)"(?: role="([^"]*)" aria-label="([^"]*)")?>([^<]*)</g,
    ),
    ([, className, role, name, text]) => [
      role ?? null,
      name ?? null,
      text,
      className.includes("emerald"),
    ],
  );
}

describe("RecipientsModal", () => {
  it("offers one visible promotion action for unread Mail", () => {
    const markup = renderModal([entry()]);

    expect(markup).toContain("In Inbox");
    expect(markup).toContain("Send now");
    expect(markup.match(/id="recipients-title"/g)).toHaveLength(1);
  });

  it("shows terminal delivery evidence without another delivery action", () => {
    const markup = renderModal([entry({ deliveryState: "accepted" })]);

    expect(markup).toContain("Sent to runtime");
    expect(markup).not.toContain("Send now");
    expect(markup).not.toContain("Retry");
  });

  it("labels ambiguous retry explicitly and exposes reply cancellation", () => {
    const markup = renderModal([entry({ deliveryState: "ambiguous" })], {
      statusKind: "reply",
      messageMode: "request_reply",
      canCancelReply: true,
    });

    expect(markup).toContain("Delivery uncertain");
    expect(markup).toContain("Retry anyway");
    expect(markup).toContain("Cancel reply request");
  });
});

it("does not color a cancelled request as replied and names each state", () => {
  const markup = renderModal(
    [
      entry({ id: "a", cleared: true, replied: true, replyRequested: true }),
      entry({ id: "b", cleared: true, cancelled: true, replyRequested: true }),
      entry({ id: "c", replyRequested: true }),
    ],
    { statusKind: "reply", messageMode: "request_reply" },
  );
  expect(statusCells(markup)).toEqual([
    ["img", "Replied", "↩", true],
    [null, null, "Cancelled", false],
    ["img", "Awaiting reply", "○", false],
  ]);
});

it("names read states in the reader's language", () => {
  const markup = renderModal([entry({ id: "a", cleared: true, read: true }), entry({ id: "b" })]);

  expect(statusCells(markup)).toEqual([
    ["img", "Read", "✓✓", true],
    ["img", "Unread", "✓", false],
  ]);
});

it("names delivery by its actual stage and does not mark an uncertain delivery as done", () => {
  const local = renderModal(
    [
      entry({ id: "a", cleared: true, deliveryState: "accepted" }),
      entry({ id: "b", cleared: true, deliveryState: "ambiguous" }),
    ],
    { statusKind: "delivery", messageMode: "send" },
  );
  expect(statusCells(local)).toEqual([
    ["img", "Sent to runtime", "✓", true],
    ["img", "Delivery uncertain", "?", false],
  ]);

  const sent = renderModal([entry({ cleared: true })], {
    statusKind: "delivery",
    messageMode: "send",
    remoteDelivery: { state: "sent" },
  });
  expect(statusCells(sent)).toEqual([["img", "Delivered to remote Group", "✓", true]]);

  const unconfirmed = renderModal([entry()], {
    statusKind: "delivery",
    messageMode: "send",
    remoteDelivery: { state: "unconfirmed" },
  });
  expect(statusCells(unconfirmed)).toEqual([["img", "Remote delivery unconfirmed", "○", false]]);
});

it("does not offer local runtime delivery for a qualified remote recipient", () => {
  const markup = renderModal([entry({ id: "peer1", label: "Remote worker" })], {
    statusKind: "delivery",
    remoteDelivery: { state: "sent" },
  });
  expect(markup).toContain("Remote worker");
  expect(markup).toContain("Delivered to remote Group");
  expect(markup).not.toContain("Send now");
  expect(markup).not.toContain("In Inbox");
  expect(markup).not.toContain("Sent to runtime");
});
