import { asRecord, failureCode } from "./codexVoiceProtocol";

const MAX_PENDING_PROVIDER_COMMANDS = 128;
const MAX_UNCONFIRMED_CONTEXTS = 1024;
const CONTEXT_ACK_TIMEOUT_MS = 30_000;
const SPEECH_START_TIMEOUT_MS = 30_000;
const OUTPUT_PREPARE_TIMEOUT_MS = 10_000;
const MAX_OUTPUT_PREPARE_RETRIES = 3;
const OUTPUT_BATCH_DELAY_MS = 150;
const MAX_OUTPUT_BATCH_CHARS = 16_000;

export type CodexVoiceOutputStatus = {
  queued: number;
  blocked: "conversation" | "speech_start" | "preparing" | "retrying" | "connection" | null;
};

function speakableText(command: unknown): string | null {
  const record = asRecord(command);
  if (
    (record?.type !== "session.context.append" && record?.type !== "delegation.context.append") ||
    record.channel !== "speakable" ||
    !Array.isArray(record.content) ||
    record.content.length !== 1
  )
    return null;
  const item = asRecord(record.content[0]);
  return item?.type === "input_text" && typeof item.text === "string" ? item.text : null;
}

function sameEnvelope(left: unknown, right: unknown): boolean {
  const a = asRecord(left);
  const b = asRecord(right);
  return (
    a?.type === b?.type &&
    a?.delegation_item_id === b?.delegation_item_id &&
    a?._cccc_result_id === b?._cccc_result_id
  );
}

export class CodexVoiceProviderChannel {
  private channel: RTCDataChannel | null = null;
  private pending: unknown[] = [];
  private unconfirmed: { type: string; sentAt: number }[] = [];
  private sent = 0;
  private acknowledged = 0;
  private speechTurnsCompleted = 0;
  private ackTimer: ReturnType<typeof setTimeout> | null = null;
  private warned = false;
  private output: unknown[] = [];
  private outputTimer: ReturnType<typeof setTimeout> | null = null;
  private speechStartTimer: ReturnType<typeof setTimeout> | null = null;
  private prepareTimer: ReturnType<typeof setTimeout> | null = null;
  private prepareAbort: AbortController | null = null;
  private prepareRetries = 0;
  private retryTimer: ReturnType<typeof setTimeout> | null = null;
  private speechStartTimeouts = 0;
  private prepareFailures = 0;
  private readonly activeTurns = new Map<string, string>();
  private preparingOutput: unknown = null;
  private rejectedResultId: string | null = null;

  constructor(
    private readonly onMessage: (data: unknown) => void,
    private readonly onFailure: (code: string) => void,
    private readonly isStopping: () => boolean,
    private readonly onUnconfirmed: () => void,
    private readonly onOutputSubmitted: (resultId: string) => void = () => {},
    private readonly prepareOutput?: (
      resultId: string,
      signal: AbortSignal,
    ) => Promise<unknown | null>,
    private readonly onStateChange: () => void = () => {},
  ) {}

  bind(channel: RTCDataChannel): void {
    this.channel = channel;
    channel.onopen = () => {
      for (const command of this.pending.splice(0)) this.send(command);
      this.scheduleOutput();
      this.onStateChange();
    };
    channel.onmessage = (event) => this.onMessage(event.data);
    channel.onerror = () => {
      if (!this.isStopping()) this.onFailure("provider_event_channel_failed");
    };
    channel.onclose = () => {
      if (!this.isStopping()) this.onFailure("provider_event_channel_closed");
    };
  }

  readyState(): RTCDataChannelState | "closed" {
    return this.channel?.readyState || "closed";
  }

  send(command: unknown, resultId?: string): void {
    if (this.isStopping()) return;
    if (resultId) command = { ...asRecord(command), _cccc_result_id: resultId };
    if (speakableText(command) !== null) {
      if (this.output.length >= MAX_UNCONFIRMED_CONTEXTS) {
        this.rejectOverflow(command);
        return;
      }
      this.output.push(command);
      this.scheduleOutput();
      this.onStateChange();
      return;
    }
    this.sendNow(command);
  }

  private sendNow(command: unknown): void {
    const channel = this.channel;
    if (!channel || channel.readyState !== "open") {
      if (this.pending.length >= MAX_PENDING_PROVIDER_COMMANDS) {
        this.rejectOverflow(command);
        return;
      }
      this.pending.push(command);
      return;
    }
    let submitted = false;
    try {
      const type = asRecord(command)?.type;
      const contextCommand =
        type === "session.context.append" || type === "delegation.context.append";
      if (contextCommand && this.unconfirmed.length >= MAX_UNCONFIRMED_CONTEXTS) {
        this.rejectOverflow(command);
        return;
      }
      const envelope = asRecord(command);
      const resultId = envelope?._cccc_result_id;
      const wireCommand = envelope ? { ...envelope } : command;
      if (asRecord(wireCommand)) delete (wireCommand as Record<string, unknown>)._cccc_result_id;
      channel.send(JSON.stringify(wireCommand));
      submitted = true;
      if (contextCommand) {
        this.sent += 1;
        this.unconfirmed.push({ type: `${type}ed`, sentAt: Date.now() });
        this.scheduleAckCheck();
      }
      if (typeof resultId === "string") this.onOutputSubmitted(resultId);
    } catch {
      // send() throwing proves this command was not submitted. Keep its source
      // available to orderly teardown before the failure callback closes us.
      if (!submitted && speakableText(command) !== null) this.output.unshift(command);
      this.onFailure("provider_command_failed");
    }
  }

  private rejectOverflow(command: unknown): void {
    // Failure synchronously initiates orderly stop. Retain the rejected ID for
    // that report without growing the bounded queue or retaining its payload.
    const id = asRecord(command)?._cccc_result_id;
    this.rejectedResultId = typeof id === "string" ? id : null;
    this.onFailure("provider_command_overflow");
  }

  observe(event: unknown): boolean {
    const record = asRecord(event);
    const type = record?.type;
    const turn = asRecord(record?.turn);
    const turnId = typeof turn?.id === "string" ? turn.id : "";
    if (
      type === "turn.created" &&
      turnId &&
      (turn?.role === "user" || turn?.role === "assistant")
    ) {
      this.activeTurns.set(turnId, turn.role);
      // From here the observed active turn, not an assumed context-to-turn
      // association, owns the conversation slot.
      if (turn.role === "assistant") this.clearSpeechStartWait();
      this.onStateChange();
    }
    if (type === "turn.done" && turnId) {
      this.activeTurns.delete(turnId);
      this.scheduleOutput();
      this.onStateChange();
    }
    if (type === "session.context.appended" || type === "delegation.context.appended") {
      // This provider acknowledges ordered context ranges, not client event IDs.
      // Count matching receipts only; a receipt is not evidence of spoken output.
      const index = this.unconfirmed.findIndex((entry) => entry.type === type);
      if (index < 0) return false;
      this.unconfirmed.splice(index, 1);
      this.acknowledged += 1;
      if (this.unconfirmed.length === 0) this.warned = false;
      this.scheduleAckCheck();
      return true;
    }
    if (type === "turn.done" && asRecord(record?.turn)?.role === "assistant") {
      this.speechTurnsCompleted += 1;
      return true;
    }
    return false;
  }

  receipt(): {
    sent: number;
    acknowledged: number;
    pending: number;
    speech_turns_completed: number;
    queued: number;
    blocked: CodexVoiceOutputStatus["blocked"];
    active_user_turns: number;
    active_assistant_turns: number;
    awaiting_speech_start: boolean;
    speech_start_timeouts: number;
    prepare_failures: number;
  } {
    return {
      sent: this.sent,
      acknowledged: this.acknowledged,
      pending: this.unconfirmed.length,
      speech_turns_completed: this.speechTurnsCompleted,
      ...this.outputStatus(),
      active_user_turns: [...this.activeTurns.values()].filter((role) => role === "user").length,
      active_assistant_turns: [...this.activeTurns.values()].filter((role) => role === "assistant")
        .length,
      awaiting_speech_start: this.speechStartTimer !== null,
      speech_start_timeouts: this.speechStartTimeouts,
      prepare_failures: this.prepareFailures,
    };
  }

  outputStatus(): CodexVoiceOutputStatus {
    return {
      queued: this.output.length + (this.preparingOutput ? 1 : 0),
      blocked: this.activeTurns.size
        ? "conversation"
        : this.speechStartTimer !== null
          ? "speech_start"
          : this.preparingOutput
            ? "preparing"
            : this.retryTimer !== null
              ? "retrying"
              : this.readyState() !== "open"
                ? "connection"
                : null,
    };
  }

  unsentResultIds(): string[] {
    return [
      ...new Set(
        [
          ...[...this.output, this.preparingOutput].map(
            (command) => asRecord(command)?._cccc_result_id,
          ),
          this.rejectedResultId,
        ].filter((id): id is string => typeof id === "string"),
      ),
    ];
  }

  private scheduleOutput(): void {
    if (
      this.outputTimer !== null ||
      this.preparingOutput ||
      this.retryTimer !== null ||
      this.speechStartTimer !== null ||
      this.activeTurns.size ||
      !this.output.length ||
      this.isStopping()
    )
      return;
    this.outputTimer = setTimeout(async () => {
      this.outputTimer = null;
      if (this.speechStartTimer !== null || this.activeTurns.size || this.isStopping()) return;
      if (this.channel?.readyState !== "open") return;
      const first = this.output.shift();
      if (!first) return;
      let text = speakableText(first)!;
      while (this.output.length && sameEnvelope(first, this.output[0])) {
        const next = speakableText(this.output[0])!;
        if (text.length + next.length > MAX_OUTPUT_BATCH_CHARS) break;
        // Projection fragments may split inside a word or a UTF-8 sentence.
        // Preserve the exact text rather than inserting punctuation/whitespace.
        text += next;
        this.output.shift();
      }
      let command: unknown = { ...asRecord(first), content: [{ type: "input_text", text }] };
      const resultId = asRecord(first)?._cccc_result_id;
      const channel = this.channel;
      if (typeof resultId === "string" && this.prepareOutput) {
        this.preparingOutput = command;
        const abort = new AbortController();
        this.prepareAbort = abort;
        this.prepareTimer = setTimeout(() => abort.abort(), OUTPUT_PREPARE_TIMEOUT_MS);
        this.onStateChange();
        try {
          const prepared = await this.prepareOutput(resultId, abort.signal);
          if (this.channel !== channel || this.isStopping()) return;
          this.prepareRetries = 0;
          if (prepared === null) {
            this.preparingOutput = null;
            this.scheduleOutput();
            this.onStateChange();
            return;
          }
          if (speakableText(prepared) === null) throw new Error("invalid prepared Voice output");
          command = { ...asRecord(prepared), _cccc_result_id: resultId };
        } catch (error) {
          if (this.channel === channel && !this.isStopping()) {
            this.output.unshift(command);
            this.preparingOutput = null;
            this.prepareFailures += 1;
            const retryable =
              abort.signal.aborted ||
              ["network_error", "daemon_unavailable"].includes(failureCode(error));
            if (retryable && this.prepareRetries < MAX_OUTPUT_PREPARE_RETRIES) {
              const delay = 1000 * 2 ** this.prepareRetries++;
              this.retryTimer = setTimeout(() => {
                this.retryTimer = null;
                this.scheduleOutput();
                this.onStateChange();
              }, delay);
            } else {
              // Only the preflight is retried. Never retry a provider send or
              // skip this source; stop and release positively unsent results.
              this.onFailure("notification_output_prepare_failed");
            }
            this.onStateChange();
          }
          return;
        } finally {
          if (this.prepareTimer !== null) clearTimeout(this.prepareTimer);
          this.prepareTimer = null;
          this.prepareAbort = null;
        }
        this.preparingOutput = null;
        // The user may have started speaking while the latest policy was read.
        if (
          this.activeTurns.size ||
          this.speechStartTimer !== null ||
          this.readyState() !== "open"
        ) {
          this.output.unshift(command);
          this.onStateChange();
          return;
        }
      }
      // This is a bounded scheduling reservation, not delivery confirmation.
      // If the provider stays silent, retain uncertainty for the submitted
      // context but let new, unsent output proceed when the conversation is idle.
      this.speechStartTimer = setTimeout(() => {
        this.speechStartTimer = null;
        if (this.isStopping()) return;
        this.speechStartTimeouts += 1;
        this.onUnconfirmed();
        this.scheduleOutput();
        this.onStateChange();
      }, SPEECH_START_TIMEOUT_MS);
      this.sendNow(command);
      this.onStateChange();
    }, OUTPUT_BATCH_DELAY_MS);
  }

  private clearSpeechStartWait(): void {
    if (this.speechStartTimer !== null) clearTimeout(this.speechStartTimer);
    this.speechStartTimer = null;
  }

  private scheduleAckCheck(): void {
    if (this.ackTimer !== null) clearTimeout(this.ackTimer);
    this.ackTimer = null;
    const oldest = this.unconfirmed[0];
    if (!oldest || this.warned || this.isStopping()) return;
    this.ackTimer = setTimeout(
      () => {
        this.ackTimer = null;
        if (this.isStopping() || this.unconfirmed.length === 0) return;
        this.warned = true;
        // Missing receipt must not trigger replay: the provider may have received
        // and even spoken this result. Keep the call available and notify the user.
        this.onUnconfirmed();
      },
      Math.max(0, CONTEXT_ACK_TIMEOUT_MS - (Date.now() - oldest.sentAt)),
    );
  }

  close(): void {
    if (this.ackTimer !== null) clearTimeout(this.ackTimer);
    this.ackTimer = null;
    this.unconfirmed = [];
    this.channel?.close();
    this.channel = null;
    this.pending = [];
    if (this.outputTimer !== null) clearTimeout(this.outputTimer);
    this.clearSpeechStartWait();
    if (this.prepareTimer !== null) clearTimeout(this.prepareTimer);
    this.prepareTimer = null;
    this.prepareAbort?.abort();
    this.prepareAbort = null;
    if (this.retryTimer !== null) clearTimeout(this.retryTimer);
    this.retryTimer = null;
    this.outputTimer = null;
    this.output = [];
    this.preparingOutput = null;
    this.rejectedResultId = null;
    this.activeTurns.clear();
    this.onStateChange();
  }
}
