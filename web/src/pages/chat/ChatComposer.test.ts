import { describe, expect, it } from "vitest";
import { readFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

import { getComposerActionVisibility, getComposerCanSend } from "./chatComposerActions";

const composerSource = readFileSync(join(dirname(fileURLToPath(import.meta.url)), "ChatComposer.tsx"), "utf8");
const mentionMenuSource = readFileSync(join(dirname(fileURLToPath(import.meta.url)), "ChatMentionMenu.tsx"), "utf8");

describe("ChatComposer action visibility", () => {
  it("hides message mode selector on small screens", () => {
    expect(getComposerActionVisibility(true)).toEqual({
      showMessageModeSelector: false,
    });
  });

  it("keeps message mode selector on larger screens", () => {
    expect(getComposerActionVisibility(false)).toEqual({
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
    expect(composerSource).toContain("getComposerGroupMentionInsertToken(selected)");
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

  it("renders remote groups as first-class To chips without remote actor picking", () => {
    expect(composerSource).toContain("remoteGroups?: GroupMeta[]");
    expect(composerSource).toContain("selectedRemoteGroupIds?: string[]");
    expect(composerSource).toContain("onToggleRemoteGroup?.(groupId)");
    expect(composerSource).toContain("remoteGroupPopoverTarget(group)");
    expect(composerSource).toContain("formatRecipientIdentifier");
    expect(composerSource).toContain("copyRecipientIdentifier");
    expect(composerSource).toContain("CopyIcon");
    expect(composerSource).toContain("onMouseEnter={(event) => showRecipientPopover(popoverTarget, event.currentTarget as HTMLElement)}");
    expect(composerSource).toContain('role="dialog"');
    expect(composerSource).toContain("getGroupRouteDisplayName(group)");
    expect(composerSource).toContain("remoteGroupSendsToForeman");
    expect(composerSource).not.toContain("remoteActors.map");
    expect(composerSource).not.toContain("remoteDetailsRef");
    expect(composerSource).not.toContain("copyRemoteGroupId");
    expect(composerSource).not.toContain("copyRemoteGroupAgentInfo");
    expect(composerSource).not.toContain("formatGroupBridgeAgentInfo");
    expect(composerSource).toContain("toTokens.length > 0 || selectedRemoteGroupIds.length > 0");
  });

  it("keeps attachment picker enabled for selected remote group chips", () => {
    expect(composerSource).toContain('if (isCrossGroup) return t(\'crossGroupAttachment\');');
    expect(composerSource).toContain('disabled={!selectedGroupId || busy === "send" || isCrossGroup}');
    expect(composerSource).not.toContain('disabled={!selectedGroupId || busy === "send" || isCrossGroup || hasRemoteGroupSelection}');
  });

  it("does not display local bridge grants as remote access levels", () => {
    expect(composerSource).toContain('const accessLevel = "messages";');
    expect(composerSource).not.toContain('String(group.federation_access_level || "").trim() || "messages"');
  });

  it("lets all To recipients expose a compact copyable identifier", () => {
    expect(composerSource).toContain("selectorPopoverTarget(tok)");
    expect(composerSource).toContain("actorPopoverTarget(actor)");
    expect(composerSource).toContain("remoteGroupPopoverTarget(group)");
    expect(composerSource).toContain("visibleRecipientPopoverTarget.identifier");
    expect(composerSource).toContain('t("copyRecipientIdentifier"');
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

  it("renders selected # and @ mentions with a clear overlay highlight", () => {
    expect(composerSource).toContain("bg-sky-400/25 px-1 text-transparent ring-1 ring-inset ring-sky-300/60");
    expect(composerSource).toContain("bg-violet-400/25 px-1 text-transparent ring-1 ring-inset ring-violet-300/60");
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
