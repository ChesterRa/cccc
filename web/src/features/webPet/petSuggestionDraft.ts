import { useComposerStore, useGroupStore, useUIStore } from "../../stores";
import type { ChatMessageData, LedgerEvent, PresentationMessageRef, ReplyTarget } from "../../types";
import { getReplyEventId } from "../../utils/chatReply";
import type { PetReminder } from "./types";
import { getPetReminderDraftText } from "./reminderText";
import { buildTaskProposalMessage } from "./taskProposal";

function truncateReplyText(value: string): string {
  const text = String(value || "").trim();
  if (!text) return "";
  return text.slice(0, 100) + (text.length > 100 ? "..." : "");
}

function findReplyTarget(groupId: string, replyTo: string): ReplyTarget {
  const gid = String(groupId || "").trim();
  const eventId = String(replyTo || "").trim();
  if (!gid || !eventId) return null;

  const state = useGroupStore.getState();
  const bucketEvents = state.chatByGroup[gid]?.events || [];
  const liveEvents = state.selectedGroupId === gid ? state.events : [];
  const events: LedgerEvent[] = [...bucketEvents, ...liveEvents];
  const match = events.find(
    (event) => String(event.id || "").trim() === eventId && String(event.kind || "").trim() === "chat.message",
  );
  if (!match) {
    return {
      eventId,
      by: "unknown",
      text: "",
    };
  }

  const replyEventId = getReplyEventId(match);
  if (!replyEventId) return null;

  const data = (match.data && typeof match.data === "object" ? match.data : {}) as ChatMessageData;
  return {
    eventId: replyEventId,
    by: String(match.by || "unknown"),
    text: truncateReplyText(String(data.text || "")),
  };
}

function hasMeaningfulComposerDraft(state: {
  composerText: string;
  composerFiles: File[];
  toText: string;
  replyTarget: ReplyTarget;
  quotedPresentationRef: PresentationMessageRef | null;
}): boolean {
  return Boolean(
    String(state.composerText || "").trim()
      || state.composerFiles.length > 0
      || String(state.toText || "").trim()
      || state.replyTarget
      || state.quotedPresentationRef,
  );
}

function mergeComposerText(existingText: string, suggestionText: string): string {
  const existing = String(existingText || "").trimEnd();
  const next = String(suggestionText || "").trim();
  if (!next) return existing;
  if (!existing) return next;
  if (existing === next || existing.endsWith(next) || existing.endsWith(`\n\n${next}`)) {
    return existing;
  }
  return `${existing}\n\n${next}`;
}

function formatComposerTaskProposalText(reminder: PetReminder): string {
  if (reminder.action.type !== "task_proposal") return "";
  const body = buildTaskProposalMessage(reminder.action);
  if (!body) return "";
  return `Pet task proposal: please ${body.charAt(0).toLowerCase()}${body.slice(1)}`;
}

function getReminderDraftPayload(
  reminder: PetReminder,
): { groupId: string; text: string; toText: string; replyTo: string } | null {
  if (reminder.action.type === "draft_message") {
    return {
      groupId: String(reminder.action.groupId || "").trim(),
      text: getPetReminderDraftText(reminder),
      toText: Array.isArray(reminder.action.to) ? reminder.action.to.join(", ") : "",
      replyTo: String(reminder.action.replyTo || "").trim(),
    };
  }
  if (reminder.action.type === "task_proposal") {
    return {
      groupId: String(reminder.action.groupId || "").trim(),
      text: formatComposerTaskProposalText(reminder),
      toText: "@foreman",
      replyTo: "",
    };
  }
  return null;
}

export function stagePetReminderDraft(reminder: PetReminder): boolean {
  const payload = getReminderDraftPayload(reminder);
  if (!payload) return false;
  const groupId = payload.groupId;
  const text = payload.text;
  if (!groupId || !text) return false;

  const groupStore = useGroupStore.getState();
  const uiStore = useUIStore.getState();
  const composerStore = useComposerStore.getState();
  const selectedGroupId = String(groupStore.selectedGroupId || "").trim();
  const isCrossGroup = selectedGroupId !== groupId;
  const targetDraft = isCrossGroup ? composerStore.drafts[groupId] || null : composerStore;
  const shouldPreserveDraft = targetDraft ? hasMeaningfulComposerDraft(targetDraft) : false;

  if (isCrossGroup) {
    composerStore.upsertDraft(groupId, (draft) => {
      const currentDraft = draft || {
        composerText: "",
        composerFiles: [],
        toText: "",
        replyTarget: null,
        quotedPresentationRef: null,
        priority: "normal" as const,
        replyRequired: false,
      };
      return {
        ...currentDraft,
        composerText: mergeComposerText(currentDraft.composerText, text),
        toText: shouldPreserveDraft ? currentDraft.toText : payload.toText,
        replyTarget: shouldPreserveDraft
          ? currentDraft.replyTarget
          : (payload.replyTo ? findReplyTarget(groupId, payload.replyTo) : null),
        quotedPresentationRef: shouldPreserveDraft ? currentDraft.quotedPresentationRef : null,
        priority: shouldPreserveDraft ? currentDraft.priority : "normal",
        replyRequired: shouldPreserveDraft ? currentDraft.replyRequired : false,
      };
    });
  }

  if (isCrossGroup) {
    groupStore.setSelectedGroupId(groupId);
  }

  uiStore.setActiveTab("chat");
  uiStore.setChatMobileSurface(groupId, "messages");
  composerStore.setDestGroupId(groupId);

  if (!isCrossGroup && !shouldPreserveDraft) {
    if (payload.replyTo) {
      composerStore.setReplyToText(payload.toText);
    } else {
      composerStore.setToText(payload.toText);
    }
    composerStore.setReplyTarget(
      payload.replyTo ? findReplyTarget(groupId, payload.replyTo) : null,
    );
    composerStore.setQuotedPresentationRef(null);
    composerStore.setPriority("normal");
    composerStore.setReplyRequired(false);
  }

  if (!isCrossGroup) {
    const currentText = useComposerStore.getState().composerText;
    composerStore.setComposerText(mergeComposerText(currentText, text));
  }
  return true;
}
