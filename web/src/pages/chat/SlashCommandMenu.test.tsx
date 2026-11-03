// @vitest-environment happy-dom
import { act } from "react";
import { createRoot } from "react-dom/client";
import { createInstance } from "i18next";
import { I18nextProvider } from "react-i18next";
import { afterEach, expect, it } from "vite-plus/test";
import chatEn from "../../i18n/locales/en/chat.json";
import chatZh from "../../i18n/locales/zh/chat.json";
import { BUILTIN_SLASH_COMMANDS } from "../../utils/builtinSlashCommands";
import type { SlashCommandItem } from "../../utils/slashCommands";
import { SlashCommandMenu } from "./SlashCommandMenu";

let root: ReturnType<typeof createRoot> | null = null;
afterEach(async () => {
  await act(async () => root?.unmount());
  document.body.innerHTML = "";
});

it("describes CCCC's own skills in the reader's language, and any other skill as it comes", async () => {
  Object.assign(globalThis, { IS_REACT_ACT_ENVIRONMENT: true });
  const i18n = createInstance();
  await i18n.init({
    lng: "zh",
    fallbackLng: "en",
    defaultNS: "chat",
    resources: { en: { chat: chatEn }, zh: { chat: chatZh } },
    interpolation: { escapeValue: false },
  });
  const selfEvolution = chatEn.slashDescriptions.selfEvolution;
  const suggestions: SlashCommandItem[] = [
    BUILTIN_SLASH_COMMANDS[0],
    {
      name: "cccc-self-evolution",
      command: "/cccc-self-evolution",
      description: selfEvolution,
      capabilityId: "skill:cccc:self-evolution",
      sourceType: "capsule_skill",
    },
    // A skill imported under a built-in id, with a description of its own, keeps it.
    {
      name: "retro",
      command: "/retro",
      description: "Our team's retro checklist.",
      capabilityId: "skill:cccc:self-evolution",
      sourceType: "capsule_skill",
    },
    {
      name: "lint",
      command: "/lint",
      description: "Run the team's linter.",
      capabilityId: "skill:acme:lint",
      sourceType: "capsule_skill",
    },
  ];
  const host = document.createElement("div");
  document.body.append(host);
  root = createRoot(host);
  await act(async () =>
    root!.render(
      <I18nextProvider i18n={i18n}>
        <SlashCommandMenu
          isDark={false}
          suggestions={suggestions}
          selectedIndex={0}
          loadMoreLabel="More"
          onSelect={() => {}}
          onHover={() => {}}
        />
      </I18nextProvider>,
    ),
  );
  const descriptions = [...host.querySelectorAll("[data-lcd-hint]")].map(
    (node) => node.textContent,
  );
  expect(descriptions).toEqual([
    chatZh.slashDescriptions.install,
    chatZh.slashDescriptions.selfEvolution,
    "Our team's retro checklist.",
    "Run the team's linter.",
  ]);
});
