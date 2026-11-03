// @vitest-environment happy-dom
import { act } from "react";
import { createRoot } from "react-dom/client";
import { createInstance } from "i18next";
import { I18nextProvider } from "react-i18next";
import { expect, it, vi } from "vite-plus/test";
import commonEn from "../i18n/locales/en/common.json";
import commonJa from "../i18n/locales/ja/common.json";
import commonZh from "../i18n/locales/zh/common.json";
import { SelectCombobox } from "./SelectCombobox";

it.each([
  ["zh", commonZh.noResults],
  ["ja", commonJa.noResults],
])("says in the reader's language (%s) that a search matched nothing", async (lng, expected) => {
  vi.stubGlobal("IS_REACT_ACT_ENVIRONMENT", true);
  const i18n = createInstance();
  await i18n.init({
    lng,
    fallbackLng: "en",
    resources: { en: { common: commonEn }, zh: { common: commonZh }, ja: { common: commonJa } },
    interpolation: { escapeValue: false },
  });
  const host = document.createElement("div");
  document.body.append(host);
  const root = createRoot(host);
  try {
    await act(async () =>
      root.render(
        <I18nextProvider i18n={i18n}>
          <SelectCombobox
            items={[{ value: "codex", label: "Codex" }]}
            value="codex"
            onChange={() => {}}
            ariaLabel="Runtime"
            searchable
          />
        </I18nextProvider>,
      ),
    );
    await act(async () =>
      host.querySelector<HTMLButtonElement>('button[role="combobox"]')!.click(),
    );
    const input = document.querySelector<HTMLInputElement>("[cmdk-input]")!;
    await act(async () => {
      Object.getOwnPropertyDescriptor(HTMLInputElement.prototype, "value")?.set?.call(input, "zzz");
      input.dispatchEvent(new Event("input", { bubbles: true }));
    });
    expect(document.body.textContent).toContain(expected);
    expect(document.body.textContent).not.toContain(commonEn.noResults);
  } finally {
    await act(async () => root.unmount());
    host.remove();
    vi.unstubAllGlobals();
  }
});
