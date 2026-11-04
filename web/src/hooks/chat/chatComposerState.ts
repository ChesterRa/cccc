import { hasComposerDraft, useComposerStore } from "../../stores/useComposerStore";
import { useGroupStore } from "../../stores";
import {
  getEffectiveComposerDestGroupId,
  isComposerGroupSettled,
} from "../../stores/useComposerStore";
import type { ComposerState, FailedSend } from "../../stores/useComposerStore";

export type FailedSendComposerSnapshot = FailedSend;
type EmptyComposerRoute = Pick<
  ComposerState,
  "activeGroupId" | "destGroupId" | "toText" | "messageMode"
>;

/** A late failure belongs to its source Group, not the Group now on screen. */
export function describeSendFailure(originGroupId: string, message: string): string {
  const groupId = originGroupId.trim();
  const { selectedGroupId, groups } = useGroupStore.getState();
  if (!groupId || selectedGroupId === groupId) return message;
  const title = groups.find((group) => group.group_id === groupId)?.title?.trim() || groupId;
  return `${title}: ${message}`;
}

/** Capture the user's actual input, including its destination, before clearing it. */
export function captureSendComposerState(
  originGroupId: string,
  id: string,
  retryKey: string | null,
  state = useComposerStore.getState(),
): FailedSend {
  return {
    id,
    originGroupId,
    retryKey,
    destGroupId: state.destGroupId || originGroupId,
    composerGroupMentionTokens: state.composerGroupMentionTokens.slice(),
    composerAgentMentionTokens: state.composerAgentMentionTokens.slice(),
    composerText: state.composerText,
    composerFiles: state.composerFiles.slice(),
    toText: state.toText,
    replyTarget: state.replyTarget,
    quotedPresentationRef: state.quotedPresentationRef,
    quotedVoiceDocumentRef: state.quotedVoiceDocumentRef,
    messageMode: state.messageMode,
  };
}

/** Consume only the draft being sent; later completion must not clear a newer draft. */
export function clearSendComposerState(originGroupId: string): EmptyComposerRoute {
  const state = useComposerStore.getState();
  state.clearComposer();
  state.clearDraft(originGroupId);
  const { activeGroupId, destGroupId, toText, messageMode } = useComposerStore.getState();
  return { activeGroupId, destGroupId, toText, messageMode };
}

export function restoreFailedSendComposerState(
  snapshot: FailedSendComposerSnapshot,
  afterClear?: EmptyComposerRoute,
): void {
  const originGroupId = String(snapshot.originGroupId || "").trim();
  if (!originGroupId) return;
  useComposerStore.getState().enqueueFailedSend(snapshot);
  const composerState = useComposerStore.getState();
  const currentSelectedGroupId = String(useGroupStore.getState().selectedGroupId || "").trim();
  if (
    afterClear &&
    currentSelectedGroupId === originGroupId &&
    !hasComposerDraft(composerState) &&
    composerState.activeGroupId === afterClear.activeGroupId &&
    composerState.destGroupId === afterClear.destGroupId &&
    composerState.toText === afterClear.toText &&
    composerState.messageMode === afterClear.messageMode
  ) {
    composerState.restoreFailedSend(originGroupId, snapshot.id);
  }
}

export type ComposerSendRoutingSnapshot = {
  selectedGroupId: string;
  destGroupId: string;
  composerGroupSettled: boolean;
  isCrossGroup: boolean;
};

export type SendMessageResponse =
  | { ok: true; result: unknown; error?: null }
  | { ok: false; result?: unknown; error: { code: string; message: string; details?: unknown } };

export function shouldRestoreComposerAfterFailedSend(successfulSendCount: number): boolean {
  return successfulSendCount === 0;
}

export function buildComposerSendRoutingSnapshot({
  selectedGroupId,
  activeGroupId,
  destGroupId,
}: {
  selectedGroupId: string;
  activeGroupId: string;
  destGroupId: string;
}): ComposerSendRoutingSnapshot {
  const selected = String(selectedGroupId || "").trim();
  const active = String(activeGroupId || "").trim();
  const dest = getEffectiveComposerDestGroupId(destGroupId, active, selected);
  const composerGroupSettled = isComposerGroupSettled(active, selected);
  return {
    selectedGroupId: selected,
    destGroupId: dest,
    composerGroupSettled,
    isCrossGroup: !!selected && !!dest && dest !== selected,
  };
}

export function parseComposerRecipientTokens(
  toText: string,
  validRecipientSet: Set<string>,
): string[] {
  const raw = String(toText || "")
    .split(",")
    .map((t) => t.trim())
    .filter((t) => t.length > 0);
  const out: string[] = [];
  const seen = new Set<string>();
  for (const token of raw) {
    if (token === "@") continue;
    if (!validRecipientSet.has(token)) continue;
    if (seen.has(token)) continue;
    seen.add(token);
    out.push(token);
  }
  return out;
}

export function buildComposerSendRecipientTokens({
  toText,
  isCrossGroup,
  validRecipientSet,
  crossGroupValidRecipientSet,
}: {
  toText: string;
  isCrossGroup: boolean;
  validRecipientSet: Set<string>;
  crossGroupValidRecipientSet: Set<string>;
}): string[] {
  return parseComposerRecipientTokens(
    toText,
    isCrossGroup ? crossGroupValidRecipientSet : validRecipientSet,
  );
}
