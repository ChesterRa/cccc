import * as api from "../../services/api";
import type {
  Actor,
  LedgerEvent,
  MessageRef,
  MessageMode,
  OptimisticAttachment,
  ReplyTarget,
} from "../../types";
import type { ComposerSendPlanTarget } from "../composerSendPlan";
import type { SendMessageResponse } from "./chatComposerState";
import { normalizeReplyMessageMode } from "../../stores/composerMessageMode";

export function buildAssistantPlaceholders(
  actors: Actor[],
  localId: string,
  groupId: string,
): LedgerEvent[] {
  const now = new Date().toISOString();
  return actors.map((actor) => ({
    id: `local:${localId}:${actor.id}`,
    ts: now,
    kind: "chat.message",
    group_id: groupId,
    by: actor.id,
    _streaming: true,
    data: {
      text: "",
      to: ["user"],
      message_mode: "send",
      stream_id: `local:${localId}:${actor.id}`,
      pending_event_id: localId,
      pending_placeholder: true,
      activities: [
        {
          id: `queued:${localId}:${actor.id}`,
          kind: "queued",
          status: "started",
          summary: "queued",
          ts: now,
        },
      ],
    },
  }));
}

export function buildOptimisticMessage(input: {
  localId: string;
  groupId: string;
  text: string;
  to: string[];
  messageMode: MessageMode;
  replyTarget: ReplyTarget;
  refs: MessageRef[];
  files: File[];
}): LedgerEvent {
  const attachments: OptimisticAttachment[] = input.files.map((file) => ({
    kind: "file",
    path: "",
    title: String(file.name || "file"),
    bytes: Number(file.size || 0),
    mime_type: String(file.type || ""),
    local_preview_url: String(URL.createObjectURL(file)),
  }));
  return {
    id: input.localId,
    kind: "chat.message",
    ts: new Date().toISOString(),
    by: "user",
    group_id: input.groupId,
    data: {
      text: input.text,
      to: input.to,
      message_mode: input.replyTarget
        ? normalizeReplyMessageMode(input.messageMode)
        : input.messageMode,
      client_id: input.localId,
      reply_to: input.replyTarget?.eventId || null,
      quote_text: input.replyTarget?.text || undefined,
      refs: input.refs,
      format: "plain",
      attachments,
      _optimistic: true,
    } as LedgerEvent["data"],
  };
}

/** Identifies where and how a prepared send goes. The daemon returns an accepted message by
 *  client ID alone, without comparing content, so a failed send's ID may be reused only for this
 *  same request from the restored draft the user has not touched; see useChatTab. */
export function retrySendKey(input: {
  groupId: string;
  text: string;
  localTo: string[];
  crossTo: string[];
  messageMode: MessageMode;
  replyTarget: ReplyTarget;
  remoteReplyGroupId: string;
  remoteReplyTo: string[];
  refs: MessageRef[];
  sendPlanTargets: ComposerSendPlanTarget[];
}): string {
  return JSON.stringify([
    input.groupId,
    input.text,
    input.localTo,
    input.crossTo,
    input.messageMode,
    // The whole target: its quote and a remote parent that a later receipt can fill in are sent.
    input.replyTarget ?? null,
    input.remoteReplyGroupId,
    input.remoteReplyTo,
    input.refs,
    input.sendPlanTargets,
  ]);
}

export async function dispatchPreparedMessage(input: {
  selectedGroupId: string;
  text: string;
  localTo: string[];
  crossTo: string[];
  files: File[];
  messageMode: MessageMode;
  localId: string;
  refs: MessageRef[];
  replyTarget: ReplyTarget;
  remoteReplyGroupId: string;
  remoteReplyTo: string[];
  sendPlanTargets: ComposerSendPlanTarget[];
  sendsCrossGroup: boolean;
  uploadSignal?: AbortSignal;
}): Promise<{ response: SendMessageResponse; successfulSendCount: number }> {
  const files = input.files.length > 0 ? input.files : undefined;
  let response: SendMessageResponse | undefined;
  let successfulSendCount = 0;
  const replyMessageMode = normalizeReplyMessageMode(input.messageMode);
  const effectiveMessageMode = input.replyTarget ? replyMessageMode : input.messageMode;
  if (input.replyTarget && input.remoteReplyGroupId) {
    const recipients = input.remoteReplyTo.length > 0 ? input.remoteReplyTo : input.crossTo;
    response = await api.sendCrossGroupMessage(
      input.selectedGroupId,
      input.remoteReplyGroupId,
      input.text,
      recipients.length > 0 ? recipients : ["@foreman"],
      replyMessageMode,
      files,
      {
        replyTo: input.replyTarget.eventId,
        quoteText: input.replyTarget.text,
        clientId: input.localId,
        remoteReplyToEventId: input.replyTarget.remoteReplyToEventId || "",
        ...(input.uploadSignal ? { signal: input.uploadSignal } : {}),
      },
    );
    if (response.ok) successfulSendCount += 1;
  } else if (input.replyTarget) {
    response = await api.replyMessage(
      input.selectedGroupId,
      input.text,
      input.localTo,
      input.replyTarget.eventId,
      files,
      input.localId,
      input.refs,
      input.replyTarget.text,
      replyMessageMode,
      ...(input.uploadSignal ? ([input.uploadSignal] as const) : []),
    );
    if (response.ok) successfulSendCount += 1;
  } else {
    const localTargets = input.sendPlanTargets.filter((target) => !target.isCrossGroup);
    for (const _target of localTargets) {
      response = await api.sendMessage(
        input.selectedGroupId,
        input.text,
        input.localTo,
        files,
        effectiveMessageMode,
        input.localId,
        input.refs,
        ...(input.uploadSignal ? ([input.uploadSignal] as const) : []),
      );
      if (!response.ok) break;
      successfulSendCount += 1;
    }
    if (!response || response.ok) {
      const crossTargets = input.sendPlanTargets.filter((target) => target.isCrossGroup);
      if (input.sendsCrossGroup && crossTargets.length === 0) {
        response = {
          ok: false,
          error: { code: "missing_cross_group_target", message: "missing cross-group target" },
        };
      }
      for (const [index, target] of crossTargets.entries()) {
        const recipients = target.recipientTokens?.length ? target.recipientTokens : input.crossTo;
        // The same ID replays a single lost-response send. If a future plan has
        // several targets, each source event needs its own deterministic ID.
        const clientId =
          input.sendPlanTargets.length === 1 ? input.localId : `${input.localId}:cross:${index}`;
        response = await api.sendCrossGroupMessage(
          input.selectedGroupId,
          target.groupId,
          input.text,
          recipients,
          effectiveMessageMode,
          files,
          { clientId, ...(input.uploadSignal ? { signal: input.uploadSignal } : {}) },
        );
        if (!response.ok) break;
        successfulSendCount += 1;
      }
    }
  }
  return {
    response: response || {
      ok: false,
      error: { code: "send_not_dispatched", message: "message send was not dispatched" },
    },
    successfulSendCount,
  };
}
