import type { VoiceHandoffContext, VoiceProvider } from "../../services/api";
import {
  processRecoveryOwnersFromDetails,
  type ProcessRecoveryOwner,
} from "../../services/api/processRecovery";
import { newClientId } from "../../utils/clientId";

const MAX_VISIBLE_TRANSCRIPT_CHARS = 4_000;
/** The most UTF-8 bytes Chet history keeps for one turn. */
const MAX_SAVED_TRANSCRIPT_BYTES = 32 * 1024;
const MAX_HANDOFF_MESSAGES = 12;
const MAX_HANDOFF_TEXT_BYTES = 8 * 1024;
const MAX_HANDOFF_SNAPSHOTS = 64;

/** A provider transcript event, unclipped; the accumulator bounds what it shows. */
export type RealtimeTranscriptUpdate = {
  role: "user" | "assistant";
  text: string;
  final: boolean;
  turnId?: string;
  replace?: boolean;
};
/** What Chet history answered for a turn offered to it. */
export type TranscriptSaveState = "pending" | "saved" | "not_saved" | "unconfirmed";
export type VoiceConversationTurn = {
  id: string;
  role: "user" | "assistant";
  text: string;
  final: boolean;
  /** The call generation, which history records as the turn's call. */
  callId?: string;
  /** Present once the turn was offered to Chet history. */
  save?: TranscriptSaveState;
};
/** One turn offered to Chet history over the call's control socket. */
export type TranscriptFrame = {
  type: "transcript";
  id: string;
  turn_id: string;
  /** Order in which this call first saw the turn, from 1. */
  seq: number;
  role: "user" | "assistant";
  text: string;
  /** The provider finished the turn; an orderly end can offer one that never did. */
  final_update: boolean;
  /** Finished and nothing clipped. */
  complete: boolean;
};
type TurnRecord = VoiceConversationTurn & {
  seq: number;
  /** The turn's own text up to the history limit; `text` is the clipped display copy. */
  raw: string;
  rawBytes: number;
  rawClipped: boolean;
  offered: boolean;
};
export const MAX_CONVERSATION_TURNS = 40;

export class RealtimeTranscriptAccumulator {
  private turns: TurnRecord[] = [];
  private historyWasTruncated = false;
  private readonly handoffContexts = new Map<string, VoiceHandoffContext>();
  private readonly activeIds: Partial<Record<RealtimeTranscriptUpdate["role"], string>> = {};
  private readonly provisionalIds: Partial<Record<RealtimeTranscriptUpdate["role"], string>> = {};
  private nextId = 0;
  private nextSeq = 0;
  private callId = "";
  /** Persona calls keep nothing in Chet history. */
  private recording = false;
  private readonly offeredTurns = new Map<string, TurnRecord>();
  private activeRole: RealtimeTranscriptUpdate["role"] | null = null;
  private readonly text: Record<RealtimeTranscriptUpdate["role"], string> = {
    user: "",
    assistant: "",
  };
  private readonly final: Record<RealtimeTranscriptUpdate["role"], boolean> = {
    user: true,
    assistant: true,
  };

  observeTurn(event: unknown): void {
    const record = asRecord(event);
    const turn = asRecord(record?.turn);
    if (
      record?.type !== "turn.created" ||
      typeof turn?.id !== "string" ||
      (turn.role !== "user" && turn.role !== "assistant")
    )
      return;
    const entry = this.ensureTurn(turn.id, turn.role, true);
    if (!entry.final) this.activeIds[turn.role] = turn.id;
  }

  /** Binds turns to their call; only an assistant call records them in Chet history. */
  bindCall(callId: string, record: boolean): void {
    this.callId = callId;
    this.recording = record;
  }

  history(): VoiceConversationTurn[] {
    return this.turns
      .filter((turn) => turn.text.trim())
      .map(({ id, role, text, final, save }) => ({
        id,
        role,
        text,
        final,
        ...(this.callId ? { callId: this.callId } : {}),
        ...(save ? { save } : {}),
      }));
  }

  /** Finished turns not yet offered to history, each offered once. */
  takeFinished(): TranscriptFrame[] {
    return this.offer((turn) => turn.final, true);
  }

  /** At an orderly end: turns the provider never finished, offered as they stand. */
  takeUnfinished(): TranscriptFrame[] {
    return this.offer((turn) => !turn.final, false);
  }

  /** Applies history's answer for one frame; returns whether a turn changed. */
  receipt(frameId: string, result: unknown): boolean {
    const turn = this.offeredTurns.get(frameId);
    if (!turn) return false;
    this.offeredTurns.delete(frameId);
    turn.save = result === "saved" || result === "not_saved" ? result : "unconfirmed";
    return true;
  }

  /** The control socket closed: frames without an answer may or may not have been kept. */
  abandonOffered(): boolean {
    if (!this.offeredTurns.size) return false;
    for (const turn of this.offeredTurns.values()) turn.save = "unconfirmed";
    this.offeredTurns.clear();
    return true;
  }

  private offer(select: (turn: TurnRecord) => boolean, finished: boolean): TranscriptFrame[] {
    if (!this.recording) return [];
    const frames: TranscriptFrame[] = [];
    for (const turn of this.turns) {
      if (turn.offered || !select(turn)) continue;
      const text = turn.raw.trim();
      if (!text) {
        // A finished turn with no words has nothing to keep.
        if (finished) turn.offered = true;
        continue;
      }
      turn.offered = true;
      turn.save = "pending";
      const frame: TranscriptFrame = {
        type: "transcript",
        id: newClientId(),
        turn_id: turn.id,
        seq: turn.seq,
        role: turn.role,
        text,
        final_update: finished,
        complete: finished && !turn.rawClipped,
      };
      this.offeredTurns.set(frame.id, turn);
      frames.push(frame);
    }
    return frames;
  }

  handoffContext(delegationId?: string): VoiceHandoffContext {
    const saved = delegationId ? this.handoffContexts.get(delegationId) : undefined;
    if (saved) return saved;
    const turns = this.history();
    const messages: VoiceHandoffContext["messages"] = [];
    let remaining = MAX_HANDOFF_TEXT_BYTES;
    let truncated = this.historyWasTruncated;
    const encoder = new TextEncoder();
    for (let index = turns.length - 1; index >= 0; index--) {
      if (messages.length === MAX_HANDOFF_MESSAGES || remaining === 0) {
        truncated = true;
        break;
      }
      const turn = turns[index];
      const bytes = encoder.encode(turn.text);
      let start = Math.max(0, bytes.length - remaining);
      // Retain a UTF-8 suffix without splitting a code point.
      while (start < bytes.length && (bytes[start] & 0xc0) === 0x80) start++;
      if (start > 0) truncated = true;
      if (start < bytes.length) {
        messages.unshift({
          role: turn.role,
          text: start ? new TextDecoder().decode(bytes.subarray(start)) : turn.text,
          incomplete: !turn.final || start > 0,
        });
        remaining -= bytes.length - start;
      }
      if (start > 0) break;
    }
    const context = { messages, truncated };
    if (delegationId) {
      this.handoffContexts.set(delegationId, context);
      if (this.handoffContexts.size > MAX_HANDOFF_SNAPSHOTS)
        this.handoffContexts.delete(this.handoffContexts.keys().next().value!);
    }
    return context;
  }

  completeTurn(id?: string, role?: RealtimeTranscriptUpdate["role"]): boolean {
    const turnId = id || (role ? this.activeIds[role] : undefined);
    if (!turnId) return role !== undefined && this.activeRole === null;
    const turn =
      this.turns.find((candidate) => candidate.id === turnId) ||
      (role ? this.ensureTurn(turnId, role, true) : undefined);
    if (!turn || (role && turn.role !== role)) return false;
    const activeId = this.activeIds[turn.role];
    const current =
      !turn.final &&
      (!activeId || activeId === turnId) &&
      (this.activeRole === null || this.activeRole === turn.role);
    turn.final = true;
    if (activeId === turnId) delete this.activeIds[turn.role];
    if (this.provisionalIds[turn.role] === turnId) delete this.provisionalIds[turn.role];
    // A delayed completion closes its own row without resetting a newer turn.
    if (current) {
      this.final[turn.role] = true;
      if (this.activeRole === turn.role) this.activeRole = null;
    }
    return current;
  }

  private ensureTurn(
    id: string,
    role: RealtimeTranscriptUpdate["role"],
    adoptProvisional = false,
  ): TurnRecord {
    let turn = this.turns.find((candidate) => candidate.id === id);
    if (!turn && adoptProvisional) {
      // The first transcript delta can precede turn.created (or only turn.done
      // may arrive). Bind that draft in place instead of leaving a prefix row.
      const provisionalId = this.provisionalIds[role];
      turn = this.turns.find((candidate) => candidate.id === provisionalId && !candidate.final);
      if (turn) {
        turn.id = id;
        if (this.activeIds[role] === provisionalId) this.activeIds[role] = id;
      }
      delete this.provisionalIds[role];
    }
    if (!turn) {
      turn = {
        id,
        role,
        text: "",
        final: false,
        seq: ++this.nextSeq,
        raw: "",
        rawBytes: 0,
        rawClipped: false,
        offered: false,
      };
      this.turns.push(turn);
      if (this.turns.length > MAX_CONVERSATION_TURNS) this.historyWasTruncated = true;
      this.turns = this.turns.slice(-MAX_CONVERSATION_TURNS);
    }
    return turn;
  }

  apply(update: RealtimeTranscriptUpdate): string {
    const role = update.role;
    const activeId = this.activeIds[role];
    const id = update.turnId || activeId || `local-${++this.nextId}`;
    const turn = this.ensureTurn(id, role, !!update.turnId);
    const whole = update.final || update.replace;
    if (whole) setRaw(turn, update.final ? update.text.trim() : update.text);
    else appendRaw(turn, update.text);
    if (
      (whole ? update.text.trim() : update.text).length > MAX_VISIBLE_TRANSCRIPT_CHARS ||
      (!update.final &&
        !update.replace &&
        turn.text.length + update.text.length > MAX_VISIBLE_TRANSCRIPT_CHARS)
    )
      this.historyWasTruncated = true;
    if (!update.turnId && !activeId) this.provisionalIds[role] = id;
    turn.text =
      update.final || update.replace
        ? boundedText(update.text)
        : boundedDelta(`${turn.text}${update.text}`);
    turn.final = update.final;
    if (update.final) {
      if (this.activeIds[role] === id) delete this.activeIds[role];
      if (this.provisionalIds[role] === id) delete this.provisionalIds[role];
    } else {
      this.activeIds[role] = id;
    }
    if (update.final) {
      this.text[role] = boundedText(update.text);
      this.final[role] = true;
      if (this.activeRole === role) this.activeRole = null;
      return this.text[role];
    }
    if (this.final[role] || this.activeRole !== role) this.text[role] = "";
    this.text[role] = update.replace
      ? boundedText(update.text)
      : boundedDelta(`${this.text[role]}${update.text}`);
    this.final[role] = false;
    this.activeRole = role;
    return this.text[role];
  }
}

export function shouldForwardProviderEvent(
  value: unknown,
  provider: VoiceProvider = "codex",
): boolean {
  return (
    !!value &&
    typeof value === "object" &&
    (value as Record<string, unknown>).type ===
      (provider === "grok" ? "response.function_call_arguments.done" : "delegation.created")
  );
}

export function realtimeTranscriptUpdate(
  value: unknown,
  provider: VoiceProvider = "codex",
): RealtimeTranscriptUpdate | null {
  if (!value || typeof value !== "object") return null;
  const event = value as Record<string, unknown>;
  const type = String(event.type || "");
  if (provider === "grok") {
    if (
      type === "conversation.item.input_audio_transcription.updated" ||
      type === "conversation.item.input_audio_transcription.completed"
    ) {
      return typeof event.transcript === "string" && typeof event.item_id === "string"
        ? {
            role: "user",
            text: event.transcript,
            final: type.endsWith("completed"),
            replace: true,
            turnId: event.item_id,
          }
        : null;
    }
    if (
      type === "response.output_audio_transcript.delta" ||
      type === "response.output_audio_transcript.done"
    ) {
      const text = type.endsWith("done") ? event.transcript : event.delta;
      return typeof text === "string" && typeof event.response_id === "string"
        ? { role: "assistant", text, final: type.endsWith("done"), turnId: event.response_id }
        : null;
    }
    return null;
  }
  if (type === "input_transcript.added" || type === "output_transcript.added") {
    const item = asRecord(event.item);
    const text = typeof item?.text === "string" ? item.text : "";
    if (!text) return null;
    return { role: type === "input_transcript.added" ? "user" : "assistant", text, final: false };
  }
  if (type !== "turn.done") return null;
  const turn = asRecord(event.turn);
  const role = turn?.role === "user" ? "user" : turn?.role === "assistant" ? "assistant" : null;
  return role && typeof turn?.transcript === "string"
    ? {
        role,
        text: turn.transcript.trim(),
        final: true,
        ...(typeof turn.id === "string" ? { turnId: turn.id } : {}),
      }
    : null;
}

function setRaw(turn: TurnRecord, text: string): void {
  const kept = utf8Prefix(text, MAX_SAVED_TRANSCRIPT_BYTES);
  turn.raw = kept.text;
  turn.rawBytes = kept.bytes;
  turn.rawClipped = kept.clipped;
}

function appendRaw(turn: TurnRecord, delta: string): void {
  if (turn.rawClipped) return;
  const kept = utf8Prefix(delta, MAX_SAVED_TRANSCRIPT_BYTES - turn.rawBytes);
  turn.raw += kept.text;
  turn.rawBytes += kept.bytes;
  turn.rawClipped = kept.clipped;
}

/** The longest prefix within `maxBytes` of UTF-8 that does not split a code point. */
function utf8Prefix(
  text: string,
  maxBytes: number,
): { text: string; bytes: number; clipped: boolean } {
  const bytes = new TextEncoder().encode(text);
  if (bytes.length <= maxBytes) return { text, bytes: bytes.length, clipped: false };
  let end = Math.max(0, maxBytes);
  while (end > 0 && (bytes[end] & 0xc0) === 0x80) end--;
  return { text: new TextDecoder().decode(bytes.subarray(0, end)), bytes: end, clipped: true };
}

export function eventStreamCloseCode(lastServerErrorCode: string): string {
  return normalizedErrorCode(lastServerErrorCode) || "event_stream_disconnected";
}

export function asRecord(value: unknown): Record<string, unknown> | null {
  return value && typeof value === "object" && !Array.isArray(value)
    ? (value as Record<string, unknown>)
    : null;
}

export function realtimeProviderError(
  value: unknown,
): { code: string; type: string; event_id: string; param: string; message: string } | null {
  const event = asRecord(value);
  if (event?.type !== "error") return null;
  const detail = asRecord(event.error) || event;
  return {
    code: providerErrorIdentifier(detail.code) || providerErrorIdentifier(event.code),
    type: providerErrorIdentifier(detail.type === "error" ? "" : detail.type),
    event_id: providerErrorIdentifier(detail.event_id) || providerErrorIdentifier(event.event_id),
    param: providerErrorIdentifier(detail.param),
    // Explanations can quote user input. Keep them bounded and in the browser,
    // never in the server diagnostic log.
    message: typeof detail.message === "string" ? detail.message.slice(0, 2_048) : "",
  };
}

function providerErrorIdentifier(value: unknown): string {
  const text = typeof value === "string" ? value.trim() : "";
  return /^[a-zA-Z0-9_.:[\]-]{1,128}$/.test(text) ? text : "";
}

export function boundedText(value: unknown): string {
  if (typeof value !== "string") return "";
  return boundedDelta(value.trim());
}

export function boundedDelta(value: unknown): string {
  if (typeof value !== "string") return "";
  let start = Math.max(0, value.length - MAX_VISIBLE_TRANSCRIPT_CHARS);
  if (start > 0 && (value.codePointAt(start - 1) ?? 0) > 0xffff) start++;
  return value.slice(start);
}

export class CodexVoiceFailure extends Error {
  readonly code: string;
  /** Owners whose unconfirmed process records block a start; empty for other failures. */
  readonly recoveryOwners: ProcessRecoveryOwner[];

  constructor(code: string, details?: unknown) {
    super(code);
    this.name = "CodexVoiceFailure";
    this.code = normalizedErrorCode(code) || "unknown";
    this.recoveryOwners =
      this.code === "codex_voice_recovery_pending" ? processRecoveryOwnersFromDetails(details) : [];
  }
}

export function failure(code: string, details?: unknown): CodexVoiceFailure {
  return new CodexVoiceFailure(code, details);
}

export function failureCode(error: unknown): string {
  return error instanceof CodexVoiceFailure ? error.code : "unknown";
}

export function failureRecoveryOwners(error: unknown): ProcessRecoveryOwner[] {
  return error instanceof CodexVoiceFailure ? error.recoveryOwners : [];
}

export function normalizedErrorCode(value: unknown): string {
  if (typeof value !== "string") return "";
  const code = value.trim().toLowerCase();
  return /^[a-z][a-z0-9_]{0,63}$/.test(code) ? code : "";
}
