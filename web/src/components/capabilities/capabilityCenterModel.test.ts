import { createInstance } from "i18next";
import { expect, it } from "vite-plus/test";
import chatEn from "../../i18n/locales/en/chat.json";
import chatJa from "../../i18n/locales/ja/chat.json";
import chatZh from "../../i18n/locales/zh/chat.json";
import settingsEn from "../../i18n/locales/en/settings.json";
import settingsJa from "../../i18n/locales/ja/settings.json";
import settingsZh from "../../i18n/locales/zh/settings.json";
import {
  BUILTIN_CAPABILITY_DESCRIPTION_KEYS,
  capabilityCenterDescription,
  capabilityCenterDisplayName,
} from "./capabilityCenterModel";

async function translator(lng: "en" | "zh" | "ja") {
  const i18n = createInstance();
  await i18n.init({
    lng,
    fallbackLng: false,
    defaultNS: "settings",
    resources: {
      en: { settings: settingsEn, chat: chatEn },
      zh: { settings: settingsZh, chat: chatZh },
      ja: { settings: settingsJa, chat: chatJa },
    },
    interpolation: { escapeValue: false },
  });
  return i18n;
}

it("has every built-in description in each language", async () => {
  for (const lng of ["en", "zh", "ja"] as const) {
    const i18n = await translator(lng);
    for (const [id, key] of BUILTIN_CAPABILITY_DESCRIPTION_KEYS) {
      expect(i18n.exists(key), `${lng} ${id} ${key}`).toBe(true);
    }
  }
});

it("describes CCCC's own capabilities by their reserved id and keeps every name", async () => {
  const t = (await translator("zh")).t;
  // The daemon's own wording need not match the locale text: the id decides.
  const pack = {
    capability_id: "pack:automation",
    name: "Automation",
    description_short: "Built-in CCCC capability pack: Automation.",
  };
  expect(capabilityCenterDescription(pack, t)).toBe(settingsZh.capabilityCenter.builtin.automation);
  // Names are what the reader searches for, and search matches the daemon's text.
  expect(capabilityCenterDisplayName(pack)).toBe("Automation");

  const skill = {
    capability_id: "skill:cccc:self-evolution",
    name: "cccc-self-evolution",
    description_short: "Description as the daemon words it.",
  };
  expect(capabilityCenterDescription(skill, t)).toBe(chatZh.slashDescriptions.selfEvolution);

  const other = {
    capability_id: "pack:acme-automation",
    name: "Acme Automation",
    description_short: "  Run the team's jobs.  ",
  };
  expect(capabilityCenterDescription(other, t)).toBe("Run the team's jobs.");
});
