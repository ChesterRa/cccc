import { useComposerStore } from "../../stores/useComposerStore";
import { useGroupStore } from "../../stores";
import {
  getEffectiveComposerDestGroupId,
  isComposerGroupSettled,
} from "../../stores/useComposerStore";
import type { PresentationMessageRef, ReplyTarget, VoiceDocumentMessageRef } from "../../types";
import type { ComposerMessageMode } from "../../stores/useComposerStore";

export type FailedSendComposerSnapshot = {
  originGroupId: string;
  composerText: string;
  composerFiles: File[];
  toText: string;
  replyTarget: ReplyTarget;
  quotedPresentationRef: PresentationMessageRef | null;
  quotedVoiceDocumentRef: VoiceDocumentMessageRef | null;
  messageMode: ComposerMessageMode;
};

type FailedSendComposerRestoreActions = Pick<
  ReturnType<typeof useComposerStore.getState>,
  | "setComposerText"
  | "setComposerFiles"
  | "setToText"
  | "setReplyTarget"
  | "setQuotedPresentationRef"
  | "setQuotedVoiceDocumentRef"
  | "setMessageMode"
  | "upsertDraft"
>;

export function restoreFailedSendComposerState(
  snapshot: FailedSendComposerSnapshot,
  actions?: FailedSendComposerRestoreActions,
): void {
  const originGroupId = String(snapshot.originGroupId || "").trim();
  if (!originGroupId) return;

  const composerState = useComposerStore.getState();
  const restoreActions = actions || composerState;
  const currentSelectedGroupId = String(useGroupStore.getState().selectedGroupId || "").trim();
  const currentActiveGroupId = String(composerState.activeGroupId || "").trim();
  const stillOnOriginGroup =
    currentSelectedGroupId === originGroupId && currentActiveGroupId === originGroupId;

  if (stillOnOriginGroup) {
    restoreActions.setComposerText(snapshot.composerText);
    restoreActions.setComposerFiles(snapshot.composerFiles);
    restoreActions.setReplyTarget(snapshot.replyTarget);
    restoreActions.setQuotedPresentationRef(snapshot.quotedPresentationRef);
    restoreActions.setQuotedVoiceDocumentRef(snapshot.quotedVoiceDocumentRef);
    restoreActions.setMessageMode(snapshot.messageMode);
    restoreActions.setToText(snapshot.toText);
    return;
  }

  restoreActions.upsertDraft(originGroupId, () => ({
    composerText: snapshot.composerText,
    composerFiles: snapshot.composerFiles,
    toText: snapshot.toText,
    replyTarget: snapshot.replyTarget,
    quotedPresentationRef: snapshot.quotedPresentationRef,
    quotedVoiceDocumentRef: snapshot.quotedVoiceDocumentRef,
    messageMode: snapshot.messageMode,
  }));
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
