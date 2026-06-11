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
    expect(composerSource).toContain("actors.map((actor)");
    expect(composerSource).toContain('val.lastIndexOf("@")');
    expect(composerSource).toContain('val.lastIndexOf("#")');
    expect(composerSource).toContain("getAgentMentionDisplayToken(selected)");
    expect(composerSource).toContain('`#${selected.label}`');
    // A `#<group>` selection inserts a delegation-context token but must NOT
    // set a cross-group destination.
    expect(composerSource).not.toContain("setDestGroupId(selected.value)");
  });

  it("does not duplicate @ for built-in recipient mentions", () => {
    expect(composerSource).toContain('label.startsWith("@") ? label : `@${label}`');
    expect(composerSource).toContain("const tokenText = getAgentMentionDisplayToken(selected)");
    expect(composerSource).toContain("setComposerText(nextText)");
  });

  it("keeps actor To chips bound to the selected group", () => {
    expect(composerSource).not.toContain("recipientActors: Actor[]");
    expect(composerSource).not.toContain("recipientChipActors.map((actor)");
    expect(composerSource).not.toContain("const recipientChipActors = isCrossGroup ? recipientActors : actors;");
    expect(composerSource).toContain("actors.map((actor)");
  });

  it("disables actor chips only while selected group actors are resolving", () => {
    expect(composerSource).toContain("const actorChipDisabled =");
    expect(composerSource).not.toContain("recipientChipActorsBusy");
    expect(composerSource).toContain('selectedGroupActorsHydrating ? "opacity-50 pointer-events-none" : ""');
    expect(composerSource).toContain("disabled={actorChipDisabled}");
  });

  it("uses selected # tokens for @ scope (bare or copied # keeps @ local)", () => {
    // Scope is decided by selected, live # tokens, not by scanning arbitrary
    // copied text that happens to contain a #group-looking substring.
    expect(composerSource).not.toContain('lastHashBeforeAt >= 0 ? "destination" : "selected"');
    expect(composerSource).not.toContain('const lastHashBeforeAt = val.lastIndexOf("#", lastAt);');
    expect(composerSource).toContain("resolveControlledComposerMentionContext({");
    expect(composerSource).toContain("setMentionActorScope(mentionCtx.scope)");
    expect(composerSource).toContain("setMentionTargetGroupId(mentionCtx.mentionTargetGroupId)");
    // A destination-scope @ (target agent) must not be added to local recipients.
    expect(composerSource).toContain('mentionScope !== "destination"');
  });

  it("resets stale mention state when no active mention trigger remains", () => {
    expect(composerSource).toMatch(/setShowMentionMenu\(false\);\s*setMentionActorScope\("selected"\);\s*setMentionTargetGroupId\(""\);\s*setMentionFilter\(""\);/);
  });

  it("treats a user # token as local delegation, never a cross-group route", () => {
    // Routing policy goes through resolveComposerHashRouting, which always pins
    // the destination to the local group.
    expect(composerSource).toContain("resolveComposerHashRouting");
    expect(composerSource).toContain("setDestGroupId(hashRouting.destGroupId)");
    // The old implicit cross-group wiring must be gone.
    expect(composerSource).not.toContain("getComposerGroupRouteDestination");
    expect(composerSource).not.toContain("setDestGroupId(nextDestGroupId)");
  });

  it("records selected # and @ mentions separately from plain copied text", () => {
    expect(composerSource).toContain("composerGroupMentionTokens");
    expect(composerSource).toContain("setComposerGroupMentionTokens");
    expect(composerSource).toContain("composerAgentMentionTokens");
    expect(composerSource).toContain("setComposerAgentMentionTokens");
    expect(composerSource).toContain("createComposerGroupMentionToken");
    expect(composerSource).toContain("createComposerAgentMentionToken");
    expect(composerSource).toContain("pruneComposerGroupMentionTokens");
    expect(composerSource).toContain("pruneComposerAgentMentionTokens");
    expect(composerSource).toContain("mentionOverlay");
  });

  it("keeps textarea fixed while only the mention overlay tracks scroll", () => {
    const textareaStart = composerSource.indexOf("<textarea");
    const textareaEnd = composerSource.indexOf("placeholder=", textareaStart);
    const textareaBlock = composerSource.slice(textareaStart, textareaEnd);
    expect(textareaBlock).not.toContain("translateY");
    expect(composerSource).toContain("transform: `translateY(-${composerScrollTop}px)`");
    expect(composerSource).toContain('className="pointer-events-none absolute inset-0 overflow-hidden');
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
