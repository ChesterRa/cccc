import { createInstance } from "i18next";
import { renderToStaticMarkup } from "react-dom/server";
import { I18nextProvider, initReactI18next } from "react-i18next";
import { beforeAll, describe, expect, it } from "vite-plus/test";

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
      resources: { en: { common: commonEn, modals: modalsEn } },
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
