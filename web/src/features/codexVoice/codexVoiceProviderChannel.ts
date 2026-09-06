import { asRecord, failureCode } from "./codexVoiceProtocol";

const MAX_PENDING_PROVIDER_COMMANDS = 128;
const MAX_UNCONFIRMED_CONTEXTS = 1024;
const CONTEXT_ACK_TIMEOUT_MS = 30_000;
const TRANSPORT_WAIT_TIMEOUT_MS = 15_000;
const MAX_BUFFERED_BYTES = 256 * 1024;
const OUTPUT_PREPARE_TIMEOUT_MS = 10_000;
const MAX_OUTPUT_PREPARE_RETRIES = 3;
const OUTPUT_BATCH_DELAY_MS = 150;
const MAX_OUTPUT_BATCH_CHARS = 16_000;
// The private Realtime protocol limits each context append to 500 tokens.
// UTF-8 bytes are a conservative token budget; enforce it after coalescing and
// preflight, where the final text can differ from the server's original chunks.
const MAX_CONTEXT_TEXT_BYTES = 500;
const utf8 = new TextEncoder();

export type CodexVoiceOutputStatus = {
  queued: number;
  blocked: "preparing" | "retrying" | "connection" | "backpressure" | null;
};

function contextText(command: unknown): string | null {
  const record = asRecord(command);
  if (
    (record?.type !== "session.context.append" && record?.type !== "delegation.context.append") ||
    !Array.isArray(record.content) ||
    record.content.length !== 1
  )
    return null;
  const item = asRecord(record.content[0]);
  return item?.type === "input_text" && typeof item.text === "string" ? item.text : null;
}

function speakableText(command: unknown): string | null {
  return asRecord(command)?.channel === "speakable" ? contextText(command) : null;
}

function contextFrames(command: unknown): unknown[] {
  const text = contextText(command);
  if (text === null || utf8.encode(text).length <= MAX_CONTEXT_TEXT_BYTES) return [command];
  const frames: unknown[] = [];
  let chunk = "";
  let bytes = 0;
  const append = () =>
    frames.push({ ...asRecord(command), content: [{ type: "input_text", text: chunk }] });
  for (const character of text) {
    const size = utf8.encode(character).length;
    if (bytes + size > MAX_CONTEXT_TEXT_BYTES) {
      append();
      chunk = "";
      bytes = 0;
    }
    chunk += character;
    bytes += size;
  }
  if (chunk) append();
  return frames;
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
  private transportTimer: ReturnType<typeof setTimeout> | null = null;
  private prepareTimer: ReturnType<typeof setTimeout> | null = null;
  private prepareAbort: AbortController | null = null;
  private prepareRetries = 0;
  private retryTimer: ReturnType<typeof setTimeout> | null = null;
  private prepareFailures = 0;
  private preparingOutput: unknown = null;
  private rejectedResultId: string | null = null;
  private ended = false;

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
    channel.bufferedAmountLowThreshold = MAX_BUFFERED_BYTES;
    channel.onbufferedamountlow = () => this.scheduleOutput();
    channel.onopen = () => {
      this.clearTransportWait();
      for (const command of this.pending.splice(0)) this.send(command);
      this.scheduleOutput();
      this.onStateChange();
    };
    channel.onmessage = (event) => this.onMessage(event.data);
    channel.onerror = () => {
      if (!this.stopping()) this.fail("provider_event_channel_failed");
    };
    channel.onclose = () => {
      if (!this.stopping()) this.fail("provider_event_channel_closed");
    };
  }

  readyState(): RTCDataChannelState | "closed" {
    return this.channel?.readyState || "closed";
  }

  send(command: unknown, resultId?: string): void {
    if (this.stopping()) return;
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
      this.waitForTransport();
      return;
    }
    let submitted = false;
    try {
      const type = asRecord(command)?.type;
      const contextCommand =
        type === "session.context.append" || type === "delegation.context.append";
      const envelope = asRecord(command);
      const resultId = envelope?._cccc_result_id;
      const wireCommand = envelope ? { ...envelope } : command;
      if (asRecord(wireCommand)) delete (wireCommand as Record<string, unknown>)._cccc_result_id;
      const frames = contextFrames(wireCommand);
      // Reserve receipt capacity for the entire output before sending any part.
      if (contextCommand && this.unconfirmed.length + frames.length > MAX_UNCONFIRMED_CONTEXTS) {
        this.rejectOverflow(command);
        return;
      }
      // Deliver context independently of speech. The provider owns conversation
      // timing and interruptions; its next turn may need this very result.
      for (const frame of frames) {
        channel.send(JSON.stringify(frame));
        submitted = true;
        if (contextCommand) {
          this.sent += 1;
          this.unconfirmed.push({ type: `${type}ed`, sentAt: Date.now() });
          this.scheduleAckCheck();
        }
      }
      // Partial submission is unknown, not fully submitted or safe to replay.
      if (typeof resultId === "string") {
        this.onOutputSubmitted(resultId);
      }
    } catch {
      // Only a failure before the first fragment proves the whole result unsent.
      // Keep that source available to orderly teardown; partial sends stay unknown.
      if (!submitted && speakableText(command) !== null) this.output.unshift(command);
      this.fail("provider_command_failed");
    }
  }

  private rejectOverflow(command: unknown): void {
    // Failure synchronously initiates orderly stop. Retain the rejected ID for
    // that report without growing the bounded queue or retaining its payload.
    const id = asRecord(command)?._cccc_result_id;
    this.rejectedResultId = typeof id === "string" ? id : null;
    this.fail("provider_command_overflow");
  }

  observe(event: unknown): boolean {
    if (this.stopping()) return false;
    const record = asRecord(event);
    const type = record?.type;
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
    buffered_bytes: number;
    prepare_failures: number;
  } {
    return {
      sent: this.sent,
      acknowledged: this.acknowledged,
      pending: this.unconfirmed.length,
      speech_turns_completed: this.speechTurnsCompleted,
      ...this.outputStatus(),
      buffered_bytes: this.channel?.bufferedAmount || 0,
      prepare_failures: this.prepareFailures,
    };
  }

  outputStatus(): CodexVoiceOutputStatus {
    return {
      queued: this.output.length + (this.preparingOutput ? 1 : 0),
      blocked: this.preparingOutput
        ? "preparing"
        : this.retryTimer !== null
          ? "retrying"
          : this.output.length
            ? this.transportBlock()
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
    if (this.preparingOutput || this.retryTimer !== null || !this.output.length || this.stopping())
      return;
    if (this.transportBlock()) {
      this.waitForTransport();
      this.onStateChange();
      return;
    }
    this.clearTransportWait();
    if (this.outputTimer !== null) return;
    this.outputTimer = setTimeout(async () => {
      this.outputTimer = null;
      if (this.stopping()) return;
      if (this.transportBlock()) {
        this.scheduleOutput();
        return;
      }
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
        let onAbort: () => void = () => {};
        const cancelled = new Promise<never>((_resolve, reject) => {
          onAbort = () => reject(new Error("Voice output preparation aborted"));
          abort.signal.addEventListener("abort", onAbort, { once: true });
        });
        this.prepareTimer = setTimeout(() => abort.abort(), OUTPUT_PREPARE_TIMEOUT_MS);
        this.onStateChange();
        try {
          // Abort requests cancellation; the race enforces the deadline even
          // if a request never settles or ignores its AbortSignal.
          const prepared = await Promise.race([
            this.prepareOutput(resultId, abort.signal),
            cancelled,
          ]);
          if (this.channel !== channel || this.stopping()) return;
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
          if (this.channel === channel && !this.stopping()) {
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
              this.fail("notification_output_prepare_failed");
            }
            this.onStateChange();
          }
          return;
        } finally {
          abort.signal.removeEventListener("abort", onAbort);
          if (this.prepareTimer !== null) clearTimeout(this.prepareTimer);
          this.prepareTimer = null;
          this.prepareAbort = null;
        }
        this.preparingOutput = null;
        // Capacity may have changed while the policy request was in flight.
        if (this.transportBlock()) {
          this.output.unshift(command);
          this.scheduleOutput();
          this.onStateChange();
          return;
        }
      }
      this.sendNow(command);
      this.scheduleOutput();
      this.onStateChange();
    }, OUTPUT_BATCH_DELAY_MS);
  }

  private transportBlock(): "connection" | "backpressure" | null {
    if (this.readyState() !== "open") return "connection";
    return (this.channel?.bufferedAmount || 0) > MAX_BUFFERED_BYTES ? "backpressure" : null;
  }

  private waitForTransport(): void {
    if (this.transportTimer !== null || this.stopping()) return;
    this.transportTimer = setTimeout(() => {
      this.transportTimer = null;
      if (this.stopping()) return;
      if (this.transportBlock()) this.fail("provider_output_stalled");
      else this.scheduleOutput();
    }, TRANSPORT_WAIT_TIMEOUT_MS);
  }

  private clearTransportWait(): void {
    if (this.transportTimer !== null) clearTimeout(this.transportTimer);
    this.transportTimer = null;
  }

  private stopping(): boolean {
    return this.ended || this.isStopping();
  }

  private fail(code: string): void {
    if (this.stopping()) return;
    this.ended = true;
    this.onFailure(code);
  }

  private scheduleAckCheck(): void {
    if (this.ackTimer !== null) clearTimeout(this.ackTimer);
    this.ackTimer = null;
    const oldest = this.unconfirmed[0];
    if (!oldest || this.warned || this.stopping()) return;
    this.ackTimer = setTimeout(
      () => {
        this.ackTimer = null;
        if (this.stopping() || this.unconfirmed.length === 0) return;
        this.warned = true;
        // Missing receipt must not trigger replay: the provider may have received
        // and even spoken this result. Keep the call available and notify the user.
        this.onUnconfirmed();
      },
      Math.max(0, CONTEXT_ACK_TIMEOUT_MS - (Date.now() - oldest.sentAt)),
    );
  }

  close(): void {
    this.ended = true;
    if (this.ackTimer !== null) clearTimeout(this.ackTimer);
    this.ackTimer = null;
    this.unconfirmed = [];
    if (this.channel) {
      this.channel.onopen = null;
      this.channel.onmessage = null;
      this.channel.onbufferedamountlow = null;
      this.channel.onerror = null;
      this.channel.onclose = null;
      this.channel.close();
    }
    this.channel = null;
    this.pending = [];
    if (this.outputTimer !== null) clearTimeout(this.outputTimer);
    this.clearTransportWait();
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
    this.onStateChange();
  }
}
