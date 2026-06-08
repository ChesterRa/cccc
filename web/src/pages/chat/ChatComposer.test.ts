import { describe, expect, it } from "vitest";
import { readFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

import { getComposerActionVisibility, getComposerCanSend } from "./chatComposerActions";

const composerSource = readFileSync(join(dirname(fileURLToPath(import.meta.url)), "ChatComposer.tsx"), "utf8");
const mentionMenuSource = readFileSync(join(dirname(fileURLToPath(import.meta.url)), "ChatMentionMenu.tsx"), "utf8");

describe("ChatComposer action visibility", () => {
  it("hides PET shortcut and message mode selector on small screens", () => {
    expect(getComposerActionVisibility(true)).toEqual({
      showPetShortcut: false,
      showMessageModeSelector: false,
    });
  });

  it("keeps PET shortcut and message mode selector on larger screens", () => {
    expect(getComposerActionVisibility(false)).toEqual({
      showPetShortcut: true,
      showMessageModeSelector: true,
    });
  });
});

describe("ChatComposer send availability", () => {
  it("enables send when the composer has non-whitespace text", () => {
    expect(getComposerCanSend({ composerText: "hello", composerFilesCount: 0 })).toBe(true);
  });

  it("enables send when the composer only has files", () => {
    expect(getComposerCanSend({ composerText: "   ", composerFilesCount: 1 })).toBe(true);
  });

  it("disables send when the composer has no text or files", () => {
    expect(getComposerCanSend({ composerText: "   ", composerFilesCount: 0 })).toBe(false);
  });

  it("keeps send available while destination actor chips are still resolving", () => {
    expect(getComposerCanSend({ composerText: "hello", composerFilesCount: 0, recipientResolutionBusy: true })).toBe(true);
    expect(getComposerCanSend({ composerText: "   ", composerFilesCount: 1, recipientResolutionBusy: true })).toBe(true);
  });
});

describe("ChatComposer destination group boundaries", () => {
  it("keeps recipient @ activation and replaces only group activation with #", () => {
    expect(composerSource).not.toContain("<GroupCombobox");
    expect(composerSource).not.toContain("getComposerDestGroupDisplayValue");
    expect(composerSource).toContain("onToggleRecipient(tok)");
    expect(composerSource).toContain("const recipientChipActors = isCrossGroup ? recipientActors : actors;");
    expect(composerSource).toContain('val.lastIndexOf("@")');
    expect(composerSource).toContain('val.lastIndexOf("#")');
    expect(composerSource).toContain("getAgentMentionDisplayToken(selected)");
    expect(composerSource).toContain('`#${selected.label}`');
    expect(composerSource).toContain("setDestGroupId(selected.value)");
  });

  it("does not duplicate @ for built-in recipient mentions", () => {
    expect(composerSource).toContain('label.startsWith("@") ? label : `@${label}`');
    expect(composerSource).toContain("setComposerText(before + getAgentMentionDisplayToken(selected) + \" \")");
  });

  it("binds actor To chips to the active send destination", () => {
    expect(composerSource).toContain("recipientActors: Actor[]");
    expect(composerSource).toContain("recipientChipActors.map((actor)");
    expect(composerSource).toContain("const recipientChipActors = isCrossGroup ? recipientActors : actors;");
  });

  it("disables actor chips only while their actor source is resolving", () => {
    expect(composerSource).toContain("const actorChipDisabled =");
    expect(composerSource).toContain("const recipientChipActorsBusy = isCrossGroup ? recipientActorsBusy : selectedGroupActorsHydrating;");
    expect(composerSource).toContain("disabled={actorChipDisabled}");
  });

  it("switches @ mention actor source only after a # route token", () => {
    expect(composerSource).toContain('const lastHashBeforeAt = val.lastIndexOf("#", lastAt);');
    expect(composerSource).toContain('setMentionActorScope(lastHashBeforeAt >= 0 ? "destination" : "selected");');
    expect(composerSource).toContain('setMentionActorScope("selected");');
  });

  it("resets stale mention state when no active mention trigger remains", () => {
    expect(composerSource).toMatch(/setShowMentionMenu\(false\);\s*setMentionActorScope\("selected"\);\s*setMentionFilter\(""\);/);
  });

  it("resets stale destination group when the # route token is removed", () => {
    expect(composerSource).toContain("getComposerGroupRouteDestination");
    expect(composerSource).toContain("setDestGroupId(nextDestGroupId)");
  });
});

describe("ChatComposer mention menu navigation", () => {
  it("keeps the active option visible and visually distinct", () => {
    expect(composerSource).toContain("<ChatMentionMenu");
    expect(mentionMenuSource).toContain('scrollIntoView({ block: "nearest" })');
    expect(mentionMenuSource).toContain("aria-selected={selected}");
    expect(mentionMenuSource).toContain("ring-black/15");
    expect(mentionMenuSource).toContain("bg-black/[0.045] text-gray-950");
    expect(mentionMenuSource).toContain("bg-white/12 text-white");
  });
});
