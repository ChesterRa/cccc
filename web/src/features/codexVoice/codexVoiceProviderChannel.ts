import { asRecord } from "./codexVoiceProtocol";

const MAX_PENDING_PROVIDER_COMMANDS = 128;
const MAX_UNCONFIRMED_CONTEXTS = 1024;
const CONTEXT_ACK_TIMEOUT_MS = 30_000;

export class CodexVoiceProviderChannel {
  private channel: RTCDataChannel | null = null;
  private pending: unknown[] = [];
  private unconfirmed: { type: string; sentAt: number }[] = [];
  private sent = 0;
  private acknowledged = 0;
  private speechTurnsCompleted = 0;
  private ackTimer: ReturnType<typeof setTimeout> | null = null;
  private warned = false;

  constructor(
    private readonly onMessage: (data: unknown) => void,
    private readonly onFailure: (code: string) => void,
    private readonly isStopping: () => boolean,
    private readonly onUnconfirmed: () => void,
  ) {}

  bind(channel: RTCDataChannel): void {
    this.channel = channel;
    channel.onopen = () => {
      for (const command of this.pending.splice(0)) this.send(command);
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

  send(command: unknown): void {
    if (this.isStopping()) return;
    const channel = this.channel;
    if (!channel || channel.readyState !== "open") {
      if (this.pending.length >= MAX_PENDING_PROVIDER_COMMANDS) {
        this.onFailure("provider_command_overflow");
        return;
      }
      this.pending.push(command);
      return;
    }
    try {
      const type = asRecord(command)?.type;
      const contextCommand =
        type === "session.context.append" || type === "delegation.context.append";
      if (contextCommand && this.unconfirmed.length >= MAX_UNCONFIRMED_CONTEXTS) {
        this.onFailure("provider_command_overflow");
        return;
      }
      channel.send(JSON.stringify(command));
      if (contextCommand) {
        this.sent += 1;
        this.unconfirmed.push({ type: `${type}ed`, sentAt: Date.now() });
        this.scheduleAckCheck();
      }
    } catch {
      this.onFailure("provider_command_failed");
    }
  }

  observe(event: unknown): boolean {
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
  } {
    return {
      sent: this.sent,
      acknowledged: this.acknowledged,
      pending: this.unconfirmed.length,
      speech_turns_completed: this.speechTurnsCompleted,
    };
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
  }
}
