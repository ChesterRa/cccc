// @vitest-environment happy-dom
import { createInstance } from "i18next";
import { act } from "react";
import { createRoot } from "react-dom/client";
import { I18nextProvider, initReactI18next } from "react-i18next";
import { afterEach, beforeEach, expect, it, vi } from "vite-plus/test";
import enChat from "../../i18n/locales/en/chat.json";
import jaChat from "../../i18n/locales/ja/chat.json";
import zhChat from "../../i18n/locales/zh/chat.json";
import { NotificationPrompt } from "./NotificationPrompt";
import { useNotificationPromptOffer } from "./useNotificationPromptOffer";

const mocks = vi.hoisted(() => ({ supported: vi.fn(() => true), status: vi.fn(), open: vi.fn() }));
vi.mock("./subscription", () => ({
  PUSH_STATUS_EVENT: "cccc:push-status",
  pushSupported: mocks.supported,
  readPushSetupStatus: mocks.status,
}));
vi.mock("../../stores/useModalStore", () => ({
  useModalStore: (select: (state: { openSettingsTarget: typeof mocks.open }) => unknown) =>
    select({ openSettingsTarget: mocks.open }),
}));

let host: HTMLDivElement;
let root: ReturnType<typeof createRoot>;

beforeEach(() => {
  (globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;
  mocks.supported.mockReset().mockReturnValue(true);
  mocks.status.mockReset();
  mocks.open.mockReset();
  window.localStorage.clear();
  host = document.createElement("div");
  document.body.append(host);
  root = createRoot(host);
});
afterEach(async () => {
  await act(async () => root.unmount());
  host.remove();
});

const buttons = () => Array.from(host.querySelectorAll("button"));

function Offer({
  waiting,
  restoreFocus = () => {},
}: {
  waiting: boolean;
  restoreFocus?: () => void;
}) {
  const offer = useNotificationPromptOffer(waiting);
  return offer.visible ? (
    <NotificationPrompt onDismiss={offer.dismiss} restoreFocus={restoreFocus} />
  ) : null;
}

it("offers notifications only while requests wait and they are off for this account", async () => {
  for (const status of ["disabled", "unbound", "keyMismatch"]) {
    mocks.status.mockResolvedValueOnce(status);
    await act(async () => root.render(<Offer waiting key={status} />));
    expect(buttons()).toHaveLength(2);
  }
  for (const status of ["enabled", "blocked", "senderUnavailable"]) {
    mocks.status.mockResolvedValueOnce(status);
    await act(async () => root.render(<Offer waiting key={status} />));
    expect(host.innerHTML).toBe("");
  }
  mocks.status.mockRejectedValueOnce(new Error("config_unavailable"));
  await act(async () => root.render(<Offer waiting key="unreadable" />));
  expect(host.innerHTML).toBe("");

  mocks.status.mockClear();
  await act(async () => root.render(<Offer waiting={false} key="idle" />));
  expect(host.innerHTML).toBe("");
  mocks.supported.mockReturnValue(false);
  await act(async () => root.render(<Offer waiting key="unsupported" />));
  expect(host.innerHTML).toBe("");
  expect(mocks.status).not.toHaveBeenCalled();
});

it("opens notification settings, and Not now keeps it hidden in this browser", async () => {
  mocks.status.mockResolvedValue("disabled");
  await act(async () => root.render(<Offer waiting />));
  await act(async () => buttons()[0].click());
  expect(mocks.open).toHaveBeenCalledWith({ scope: "global", tab: "notifications" });

  await act(async () => buttons()[1].click());
  expect(host.innerHTML).toBe("");
  await act(async () => root.render(<Offer waiting key="again" />));
  expect(host.innerHTML).toBe("");
});

it("follows settings when notifications are turned on there", async () => {
  mocks.status.mockResolvedValueOnce("disabled");
  await act(async () => root.render(<Offer waiting />));
  expect(buttons()).toHaveLength(2);
  mocks.status.mockResolvedValueOnce("enabled");
  await act(async () => window.dispatchEvent(new Event("cccc:push-status")));
  expect(host.innerHTML).toBe("");
});

it("moves focus on when the offer closes with focus inside it, and only then", async () => {
  const restore = vi.fn();
  mocks.status.mockResolvedValue("disabled");
  await act(async () => root.render(<Offer waiting restoreFocus={restore} />));
  buttons()[1].focus();
  await act(async () => buttons()[1].click());
  expect(host.innerHTML).toBe("");
  expect(restore).toHaveBeenCalledTimes(1);

  // The offer also closes on its own when the last request is answered elsewhere.
  window.localStorage.clear();
  await act(async () => root.render(<Offer waiting key="again" restoreFocus={restore} />));
  buttons()[0].focus();
  await act(async () => root.render(<Offer waiting={false} key="again" restoreFocus={restore} />));
  expect(restore).toHaveBeenCalledTimes(2);

  const elsewhere = document.createElement("button");
  document.body.append(elsewhere);
  await act(async () => root.render(<Offer waiting key="third" restoreFocus={restore} />));
  elsewhere.focus();
  await act(async () => root.render(<Offer waiting={false} key="third" restoreFocus={restore} />));
  expect(restore).toHaveBeenCalledTimes(2);
  expect(document.activeElement).toBe(elsewhere);
  elsewhere.remove();
});

it("starts with its call to action in bold in every language", async () => {
  const i18n = createInstance();
  await i18n
    .use(initReactI18next)
    .init({
      lng: "en",
      fallbackLng: "en",
      defaultNS: "chat",
      resources: { en: { chat: enChat }, zh: { chat: zhChat }, ja: { chat: jaChat } },
      interpolation: { escapeValue: false },
    });
  for (const language of ["en", "zh", "ja"]) {
    await act(async () => i18n.changeLanguage(language));
    await act(async () =>
      root.render(
        <I18nextProvider i18n={i18n}>
          <NotificationPrompt onDismiss={() => {}} restoreFocus={() => {}} />
        </I18nextProvider>,
      ),
    );
    const lead = host.querySelector("p strong")?.textContent ?? "";
    expect(lead, language).not.toBe("");
    expect(host.querySelector("p")?.textContent?.startsWith(lead), language).toBe(true);
    expect(host.textContent, language).not.toContain("<");
  }
});

it("follows Not now from another tab of this browser", async () => {
  mocks.status.mockResolvedValue("disabled");
  await act(async () => root.render(<Offer waiting />));
  expect(buttons()).toHaveLength(2);
  window.localStorage.setItem("cccc.notificationPrompt.dismissed", "1");
  await act(async () =>
    window.dispatchEvent(new StorageEvent("storage", { key: "cccc.notificationPrompt.dismissed" })),
  );
  expect(host.innerHTML).toBe("");
});
