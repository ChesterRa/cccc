const MAX_VISIBLE_TRANSCRIPT_CHARS = 4_000;

export type RealtimeTranscriptUpdate = { role: "user" | "assistant"; text: string; final: boolean };

export class RealtimeTranscriptAccumulator {
  private activeRole: RealtimeTranscriptUpdate["role"] | null = null;
  private readonly text: Record<RealtimeTranscriptUpdate["role"], string> = {
    user: "",
    assistant: "",
  };
  private readonly final: Record<RealtimeTranscriptUpdate["role"], boolean> = {
    user: true,
    assistant: true,
  };

  apply(update: RealtimeTranscriptUpdate): string {
    const role = update.role;
    if (update.final) {
      this.text[role] = boundedText(update.text);
      this.final[role] = true;
      if (this.activeRole === role) this.activeRole = null;
      return this.text[role];
    }
    if (this.final[role] || this.activeRole !== role) this.text[role] = "";
    this.text[role] = boundedDelta(`${this.text[role]}${update.text}`);
    this.final[role] = false;
    this.activeRole = role;
    return this.text[role];
  }
}

export function shouldForwardProviderEvent(value: unknown): boolean {
  return (
    !!value &&
    typeof value === "object" &&
    (value as Record<string, unknown>).type === "delegation.created"
  );
}

export function realtimeTranscriptUpdate(value: unknown): RealtimeTranscriptUpdate | null {
  if (!value || typeof value !== "object") return null;
  const event = value as Record<string, unknown>;
  const type = String(event.type || "");
  if (type === "input_transcript.added" || type === "output_transcript.added") {
    const item = asRecord(event.item);
    const text = boundedDelta(item?.text);
    if (!text) return null;
    return { role: type === "input_transcript.added" ? "user" : "assistant", text, final: false };
  }
  if (type !== "turn.done") return null;
  const turn = asRecord(event.turn);
  const role = turn?.role === "user" ? "user" : turn?.role === "assistant" ? "assistant" : null;
  const text = boundedText(turn?.transcript);
  return role && typeof turn?.transcript === "string" ? { role, text, final: true } : null;
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
  const text = value.trim();
  if (text.length <= MAX_VISIBLE_TRANSCRIPT_CHARS) return text;
  return text.slice(text.length - MAX_VISIBLE_TRANSCRIPT_CHARS);
}

export function boundedDelta(value: unknown): string {
  if (typeof value !== "string") return "";
  return value.length <= MAX_VISIBLE_TRANSCRIPT_CHARS
    ? value
    : value.slice(value.length - MAX_VISIBLE_TRANSCRIPT_CHARS);
}

export class CodexVoiceFailure extends Error {
  readonly code: string;

  constructor(code: string) {
    super(code);
    this.name = "CodexVoiceFailure";
    this.code = normalizedErrorCode(code) || "unknown";
  }
}

export function failure(code: string): CodexVoiceFailure {
  return new CodexVoiceFailure(code);
}

export function failureCode(error: unknown): string {
  return error instanceof CodexVoiceFailure ? error.code : "unknown";
}

export function normalizedErrorCode(value: unknown): string {
  if (typeof value !== "string") return "";
  const code = value.trim().toLowerCase();
  return /^[a-z][a-z0-9_]{0,63}$/.test(code) ? code : "";
}
