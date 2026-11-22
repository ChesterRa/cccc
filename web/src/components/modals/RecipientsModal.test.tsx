import { createInstance } from "i18next";
import { renderToStaticMarkup } from "react-dom/server";
import { I18nextProvider, initReactI18next } from "react-i18next";
import { beforeAll, describe, expect, it } from "vite-plus/test";

import chatEn from "../../i18n/locales/en/chat.json";
import commonEn from "../../i18n/locales/en/common.json";
import modalsEn from "../../i18n/locales/en/modals.json";
import modalsJa from "../../i18n/locales/ja/modals.json";
import modalsZh from "../../i18n/locales/zh/modals.json";
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

  it("says why a delivery failed or is uncertain, and only then", () => {
    const reason = "The agent exited (code 1); check its terminal, then retry";

    expect(renderModal([entry({ deliveryState: "failed", deliveryReason: reason })])).toContain(
      reason,
    );
    expect(renderModal([entry({ deliveryState: "ambiguous", deliveryReason: reason })])).toContain(
      reason,
    );
    expect(
      renderModal([entry({ deliveryState: "accepted", deliveryReason: reason })]),
    ).not.toContain(reason);
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

it("shows Chet by name with only its reply state, and nothing to deliver", () => {
  const markup = renderModal([entry({ id: "svc:chet", label: "Chet", replyRequested: true })], {
    statusKind: "reply",
    messageMode: "request_reply",
    toLabel: "Chet",
    canCancelReply: true,
  });
  expect(markup).toContain(">Chet<");
  expect(markup).not.toContain("svc:chet<");
  // Chet takes messages in its own panel; the Group cannot deliver to it or report delivery.
  expect(markup).not.toContain(modalsEn.recipients.sendNow);
  expect(markup).not.toContain(modalsEn.recipients.notDelivered);
});

describe("delivery evidence and waiting reasons", () => {
  it("distinguishes terminal_written, channel_accepted, and legacy accepted without guessing", () => {
    const markup = renderModal(
      [
        entry({
          id: "pty",
          cleared: true,
          deliveryState: "accepted",
          deliveryEvidence: "terminal_written",
        }),
        entry({
          id: "channel",
          cleared: true,
          deliveryState: "accepted",
          deliveryEvidence: "channel_accepted",
        }),
        entry({ id: "legacy", cleared: true, deliveryState: "accepted" }),
      ],
      { statusKind: "delivery", messageMode: "send" },
    );

    expect(statusCells(markup)).toEqual([
      ["img", "Written to terminal (execution unconfirmed)", "✓", true],
      ["img", "Accepted by the runtime", "✓", true],
      ["img", "Sent to runtime", "✓", true],
    ]);
    expect(markup).not.toContain("Send now");
    expect(markup).not.toContain("Retry");
  });

  it("shows waiting reasons for unsubmitted or paused recipients instead of terminal failure", () => {
    const markup = renderModal(
      [
        entry({ id: "a", waitingReason: "group_paused" }),
        entry({ id: "b", waitingReason: "group_stopped" }),
        entry({ id: "c", waitingReason: "member_disabled" }),
        entry({ id: "d", waitingReason: "runtime_switch" }),
        entry({
          id: "e",
          deliveryState: "failed",
          waitingReason: "not_submitted",
          deliveryReason: "held while switching",
        }),
        entry({ id: "f", waitingReason: "future_reason" }),
        entry({ id: "g", waitingReason: "toString" }),
      ],
      { statusKind: "delivery", messageMode: "send" },
    );

    expect(statusCells(markup)).toEqual([
      ["img", "Waiting · Group paused", "○", false],
      ["img", "Waiting · Group stopped", "○", false],
      ["img", "Waiting · Member disabled", "○", false],
      ["img", "Waiting · Switching runtime", "○", false],
      ["img", "Waiting · Not submitted yet", "○", false],
      ["img", "Waiting", "○", false],
      ["img", "Waiting", "○", false],
    ]);
    expect(markup).toContain("held while switching");
    expect(markup).not.toContain("Delivery failed");
    expect(markup).not.toContain("Retry");
    expect(markup).toContain("Send now");

    const disabledOnly = renderModal(
      [
        entry({ id: "c", waitingReason: "member_disabled" }),
        entry({ id: "h", deliveryState: "failed", waitingReason: "member_disabled" }),
      ],
      { statusKind: "delivery", messageMode: "send" },
    );
    expect(disabledOnly).toContain("Waiting · Member disabled");
    expect(disabledOnly).not.toContain("Send now");
    expect(disabledOnly).not.toContain("Retry");

    // Read Mail does not show a waiting or failure label even if a stale delivery state lingers.
    const readMail = renderModal(
      [
        entry({
          id: "m",
          cleared: true,
          read: true,
          deliveryState: "failed",
          waitingReason: "not_submitted",
        }),
      ],
      { statusKind: "read", messageMode: "mail" },
    );
    expect(readMail).not.toContain("Waiting");
    expect(readMail).not.toContain("Delivery failed");
    expect(readMail).toContain("In Inbox");
  });

  it("keeps en, zh, and ja delivery evidence and waiting reason keys in parity", () => {
    const waitingKeys = [
      "groupPaused",
      "groupStopped",
      "memberDisabled",
      "runtimeSwitch",
      "notSubmitted",
    ] as const;
    for (const locale of [modalsEn, modalsZh, modalsJa]) {
      expect(locale.recipients.deliveryTerminalWritten).toBeTruthy();
      expect(locale.recipients.deliveryChannelAccepted).toBeTruthy();
      expect(locale.recipients.deliveryWaiting).toBeTruthy();
      for (const key of waitingKeys) {
        expect(locale.recipients.waitingReason[key]).toBeTruthy();
      }
    }
  });
});
