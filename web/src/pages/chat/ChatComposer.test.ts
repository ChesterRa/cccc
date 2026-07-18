import { describe, expect, it } from "vite-plus/test";
import { readFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

import { getComposerActionVisibility, getComposerCanSend } from "./chatComposerActions";
import { RECIPIENT_POPOVER_GAP_PX } from "./useRecipientPopover";

const composerSource = readFileSync(
  join(dirname(fileURLToPath(import.meta.url)), "ChatComposer.tsx"),
  "utf8",
);
const compactComposerSource = composerSource.replace(/\s+/g, " ");
const recipientRowSource = readFileSync(
  join(dirname(fileURLToPath(import.meta.url)), "ComposerRecipientsRow.tsx"),
  "utf8",
);
const recipientPopoverSource = readFileSync(
  join(dirname(fileURLToPath(import.meta.url)), "RecipientPopover.tsx"),
  "utf8",
);
const recipientPopoverHookSource = readFileSync(
  join(dirname(fileURLToPath(import.meta.url)), "useRecipientPopover.ts"),
  "utf8",
);
const recipientSources = [
  recipientRowSource,
  recipientPopoverSource,
  recipientPopoverHookSource,
].join("\n");
const mentionMenuSource = readFileSync(
  join(dirname(fileURLToPath(import.meta.url)), "ChatMentionMenu.tsx"),
  "utf8",
);

describe("ChatComposer action visibility", () => {
  it("hides message mode selector on small screens", () => {
    expect(getComposerActionVisibility(true)).toEqual({ showMessageModeSelector: false });
  });

  it("keeps message mode selector on larger screens", () => {
    expect(getComposerActionVisibility(false)).toEqual({ showMessageModeSelector: true });
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
    expect(
      getComposerCanSend({
        composerText: "hello",
        composerFilesCount: 0,
        recipientResolutionBusy: true,
      }),
    ).toBe(true);
    expect(
      getComposerCanSend({
        composerText: "   ",
        composerFilesCount: 1,
        recipientResolutionBusy: true,
      }),
    ).toBe(true);
  });
});

describe("ChatComposer destination group boundaries", () => {
  it("delegates recipient chips and details to a focused recipient row", () => {
    expect(composerSource).toContain("<ComposerRecipientsRow");
    expect(composerSource).not.toContain("actors.map((actor)");
    expect(composerSource).not.toContain("visibleRecipientPopoverTarget.identifier");
  });

  it("keeps recipient @ activation and replaces only group activation with #", () => {
    expect(composerSource).not.toContain("<GroupCombobox");
    expect(composerSource).not.toContain("getComposerDestGroupDisplayValue");
    expect(recipientRowSource).toContain("onToggleRecipient(token)");
    expect(recipientRowSource).toContain("actors.map((actor)");
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
    expect(composerSource).not.toContain(
      "const recipientChipActors = isCrossGroup ? recipientActors : actors;",
    );
    expect(recipientRowSource).toContain("actors.map((actor)");
  });

  it("renders remote groups as first-class To chips without remote actor picking", () => {
    expect(composerSource).toContain("remoteGroups?: GroupMeta[]");
    expect(composerSource).toContain("selectedRemoteGroupIds?: string[]");
    expect(recipientRowSource).toContain("onToggleRemoteGroup?.(groupId)");
    expect(recipientRowSource).toContain("popover.remoteGroupTarget(group)");
    expect(recipientPopoverHookSource).toContain("formatRecipientIdentifier");
    expect(recipientPopoverHookSource).toContain("copyIdentifier");
    expect(recipientPopoverSource).toContain("CopyIcon");
    expect(recipientRowSource).toMatch(
      /onMouseEnter=\{\(event\) => popover\.show\(target, event\.currentTarget\)\}/,
    );
    expect(recipientPopoverSource).toContain('role="dialog"');
    expect(recipientPopoverHookSource).toContain("getGroupRouteDisplayName(group)");
    expect(recipientSources).not.toContain("remoteGroupSendsToForeman");
    expect(recipientSources).not.toContain("remoteActors.map");
    expect(recipientSources).not.toContain("remoteDetailsRef");
    expect(recipientSources).not.toContain("copyRemoteGroupId");
    expect(recipientSources).not.toContain("copyRemoteGroupAgentInfo");
    expect(recipientSources).not.toContain("formatGroupBridgeAgentInfo");
    expect(recipientRowSource).toContain(
      "toTokens.length > 0 || selectedRemoteGroupIds.length > 0",
    );
  });

  it("keeps attachment picker enabled for selected remote group chips", () => {
    expect(compactComposerSource).toContain('if (isCrossGroup) return t("crossGroupAttachment");');
    expect(composerSource).toContain(
      'disabled={!selectedGroupId || busy === "send" || isCrossGroup}',
    );
    expect(composerSource).not.toContain(
      'disabled={!selectedGroupId || busy === "send" || isCrossGroup || hasRemoteGroupSelection}',
    );
  });

  it("does not display local bridge grants as remote access levels", () => {
    expect(recipientSources).toContain(
      'String(group.group_bridge_access_level || "").trim() || "unknown"',
    );
    expect(recipientPopoverHookSource).toContain('t("remoteGroupAccessUnknown"');
    expect(recipientSources).not.toContain('const accessLevel = "messages";');
  });

  it("lets all To recipients expose a compact copyable identifier", () => {
    expect(recipientRowSource).toContain("popover.selectorTarget(token)");
    expect(recipientRowSource).toContain("popover.actorTarget(actor)");
    expect(recipientRowSource).toContain("popover.remoteGroupTarget(group)");
    expect(recipientPopoverSource).toContain("target.identifier");
    expect(recipientPopoverSource).toContain("target.kindLabel");
    expect(recipientPopoverSource).toContain("target.badgeLabel");
    expect(recipientPopoverSource).toContain('t("copyRecipientIdentifier"');
    expect(recipientPopoverSource).not.toContain("target.detail");
    expect(recipientPopoverSource).not.toContain("target.idLabel");
  });

  it("disables actor chips only while selected group actors are resolving", () => {
    expect(recipientRowSource).toContain("const actorChipDisabled =");
    expect(recipientRowSource).not.toContain("recipientChipActorsBusy");
    expect(recipientRowSource).toContain(
      'selectedGroupActorsHydrating ? "opacity-50 pointer-events-none" : ""',
    );
    expect(recipientRowSource).toContain("disabled={actorChipDisabled}");
  });

  it("uses selected # tokens for @ suggestions without changing recipients", () => {
    // Scope is decided by selected, live # tokens, not by scanning arbitrary
    // copied text that happens to contain a #group-looking substring.
    expect(composerSource).not.toContain('lastHashBeforeAt >= 0 ? "destination" : "selected"');
    expect(composerSource).not.toContain('const lastHashBeforeAt = val.lastIndexOf("#", lastAt);');
    expect(composerSource).toContain("resolveControlledComposerMentionContext({");
    expect(composerSource).toContain("setMentionActorScope(mentionCtx.scope)");
    expect(composerSource).toContain("setMentionTargetGroupId(mentionCtx.mentionTargetGroupId)");
    // @ mentions are text references/autocomplete only; recipient chips own routing.
    expect(composerSource).not.toContain("onAppendRecipientToken");
    expect(composerSource).not.toContain("toTokens.includes(selected.value)");
  });

  it("resets stale mention state when no active mention trigger remains", () => {
    expect(composerSource).toMatch(
      /setShowMentionMenu\(false\);\s*setMentionActorScope\("selected"\);\s*setMentionTargetGroupId\(""\);\s*setMentionFilter\(""\);/,
    );
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
    expect(composerSource).toContain(
      "bg-sky-400/25 px-1 text-transparent ring-1 ring-inset ring-sky-300/60",
    );
    expect(composerSource).toContain(
      "bg-violet-400/25 px-1 text-transparent ring-1 ring-inset ring-violet-300/60",
    );
  });

  it("keeps textarea fixed while only the mention overlay tracks scroll", () => {
    const textareaStart = composerSource.indexOf("<textarea");
    const textareaEnd = composerSource.indexOf("placeholder=", textareaStart);
    const textareaBlock = composerSource.slice(textareaStart, textareaEnd);
    expect(textareaBlock).not.toContain("translateY");
    expect(composerSource).toContain("transform: `translateY(-${composerScrollTop}px)`");
    expect(composerSource).toContain(
      'className="pointer-events-none absolute inset-0 overflow-hidden',
    );
  });

  it("hides the focus ring only on the main message textarea", () => {
    const textareaStart = composerSource.indexOf("<textarea");
    const textareaEnd = composerSource.indexOf("placeholder=", textareaStart);
    const textareaBlock = composerSource.slice(textareaStart, textareaEnd);
    expect(textareaBlock).toContain("focus-visible:shadow-none");
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
    expect(mentionMenuSource).toContain("item.badgeKind");
    expect(mentionMenuSource).toContain('t("remoteBadge"');
  });

  it("keeps recipient hover popovers compact", () => {
    expect(recipientPopoverHookSource).toContain(
      "Math.min(196, Math.max(176, viewportWidth - 16))",
    );
  });

  it("positions recipient hover popovers above their chips", () => {
    expect(RECIPIENT_POPOVER_GAP_PX).toBe(6);
    expect(recipientPopoverHookSource).toContain("const top = rect.top");
    expect(recipientPopoverHookSource).toContain(
      "const transform = `translateY(calc(-100% - ${RECIPIENT_POPOVER_GAP_PX}px))`",
    );
    expect(recipientPopoverHookSource).toContain("setStyle({ top, left: 8, right: 8, transform })");
    expect(recipientPopoverHookSource).not.toContain("const top = rect.bottom + 6");
  });

  it("keeps the whole recipient popover hoverable without covering its chip", () => {
    expect(recipientPopoverSource).toContain("fixed pointer-events-auto z-[1000]");
    expect(recipientPopoverSource).not.toContain("fixed pointer-events-none z-[1000]");
    expect(recipientPopoverSource).toContain('className="absolute inset-x-0 top-full"');
    expect(recipientPopoverSource).toContain("style={{ height: RECIPIENT_POPOVER_GAP_PX }}");
    expect(recipientPopoverSource).toContain("onMouseEnter={onCancelHide}");
    expect(recipientPopoverSource).toContain("onMouseLeave={onScheduleHide}");
  });

  it("keeps the recipient popover alive while its copy action has focus", () => {
    expect(recipientPopoverSource).toContain("onFocusCapture={onCancelHide}");
    expect(recipientPopoverSource).toContain("onBlurCapture={onScheduleHide}");
  });
});

describe("ChatComposer message history wiring", () => {
  it("keeps menu navigation ahead of empty-composer history recall", () => {
    const slashNavigation = composerSource.indexOf(
      "if (showSlashMenu && visibleSlashSuggestions.length > 0)",
    );
    const mentionNavigation = composerSource.indexOf(
      "if (showMentionMenu && mentionSuggestions.length > 0)",
    );
    const historyRecall = composerSource.indexOf("canStartComposerHistory({");

    expect(slashNavigation).toBeGreaterThan(-1);
    expect(mentionNavigation).toBeGreaterThan(slashNavigation);
    expect(historyRecall).toBeGreaterThan(mentionNavigation);
  });

  it("uses the unfiltered live source while excluding stale routing semantics", () => {
    expect(compactComposerSource).toContain(
      "buildComposerHistoryEntries(suggestionSourceMessages || recentMessages)",
    );
    expect(compactComposerSource).toContain("setComposerGroupMentionTokens([])");
    expect(compactComposerSource).toContain("setComposerAgentMentionTokens([])");
  });

  it("leaves history mode on pointer edits and protects IME composition", () => {
    expect(composerSource).toContain("onPointerDown={exitComposerHistory}");
    expect(compactComposerSource).toContain(
      "nativeKeyboardEvent.isComposing || nativeKeyboardEvent.keyCode === 229",
    );
  });

  it("consumes Escape while leaving history mode before reply cancellation", () => {
    expect(compactComposerSource).toContain(
      'if (historySession) { if (e.key === "Escape") { e.preventDefault(); exitComposerHistory(); return; }',
    );
  });
});
