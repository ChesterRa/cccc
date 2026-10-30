// @vitest-environment happy-dom
import { createInstance } from "i18next";
import { act } from "react";
import { createRoot } from "react-dom/client";
import { I18nextProvider, initReactI18next } from "react-i18next";
import { expect, it, vi } from "vite-plus/test";

import chatEn from "../i18n/locales/en/chat.json";
import chatJa from "../i18n/locales/ja/chat.json";
import chatZh from "../i18n/locales/zh/chat.json";
import { apiJson } from "../services/api";
import type { Actor } from "../types";
import { SearchModal } from "./SearchModal";

vi.mock("../services/api", () => ({ apiJson: vi.fn() }));

it.each([
  ["en", chatEn, "Reply to Review lead", "Reply to User", "Reply to System", "Reply"],
  ["zh", chatZh, "回复 Review lead", "回复 用户", "回复 系统", "回复"],
  ["ja", chatJa, "Review leadに返信", "ユーザーに返信", "システムに返信", "返信"],
])(
  "localizes search result reply labels in %s",
  async (language, chat, worker, user, system, empty) => {
    const i18n = createInstance();
    await i18n
      .use(initReactI18next)
      .init({
        lng: language,
        fallbackLng: language,
        defaultNS: "chat",
        resources: { [language]: { chat } },
        interpolation: { escapeValue: false },
      });
    Object.assign(globalThis, { IS_REACT_ACT_ENVIRONMENT: true });
    const host = document.createElement("div");
    document.body.append(host);
    const root = createRoot(host);
    try {
      await act(async () => {
        root.render(
          <I18nextProvider i18n={i18n}>
            <SearchModal
              isOpen
              groupId="fixture"
              actors={[{ id: "worker", title: "Review lead" } as Actor]}
              isDark={false}
              onClose={() => {}}
              onReply={() => {}}
            />
          </I18nextProvider>,
        );
      });
      vi.mocked(apiJson).mockResolvedValueOnce({
        ok: true,
        result: {
          events: ["worker", "user", "system", ""].map((by, index) => ({
            id: `fixture-${index}`,
            kind: "chat.message",
            ts: "2026-09-17T00:00:00Z",
            by,
            data: { text: "Example" },
          })),
          has_more: false,
        },
      });
      await act(async () => {
        host
          .querySelector("form")!
          .dispatchEvent(new Event("submit", { bubbles: true, cancelable: true }));
      });
      const resultLabels = Array.from(host.querySelectorAll("article button[aria-label]")).map(
        (button) => button.getAttribute("aria-label"),
      );
      for (const label of [worker, user, system, empty]) expect(resultLabels).toContain(label);
    } finally {
      await act(async () => root.unmount());
      host.remove();
      vi.mocked(apiJson).mockReset();
    }
  },
);
