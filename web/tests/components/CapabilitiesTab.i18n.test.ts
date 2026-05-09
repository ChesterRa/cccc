import { describe, expect, it } from "vitest";

import enChat from "../../src/i18n/locales/en/chat.json";
import enSettings from "../../src/i18n/locales/en/settings.json";
import jaChat from "../../src/i18n/locales/ja/chat.json";
import zhChat from "../../src/i18n/locales/zh/chat.json";
import zhSettings from "../../src/i18n/locales/zh/settings.json";

describe("CapabilitiesTab i18n", () => {
  it("defines imported skill assignment management labels in supported settings locales", () => {
    const requiredKeys = [
      "manageSkillAssignments",
      "manageAssignmentsTitle",
      "manageAssignmentsSubtitle",
    ] as const;

    for (const locale of [enSettings, zhSettings]) {
      for (const key of requiredKeys) {
        expect(locale.capabilities[key]).toBeTruthy();
      }
    }
  });

  it("defines slash command labels in supported chat locales", () => {
    const requiredKeys = [
      "slashCommandAttachmentUnsupported",
      "slashCommandReplyUnsupported",
      "slashCommandQuotedPresentationUnsupported",
      "slashCommandCrossGroupUnsupported",
      "slashCommandMissingArgs",
      "slashCommandLoadMore",
    ] as const;

    for (const locale of [enChat, zhChat, jaChat]) {
      for (const key of requiredKeys) {
        expect(locale[key]).toBeTruthy();
      }
    }
  });
});
