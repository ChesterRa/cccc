const MAX_PENDING_PROVIDER_COMMANDS = 128;

export class CodexVoiceProviderChannel {
  private channel: RTCDataChannel | null = null;
  private pending: unknown[] = [];

  constructor(
    private readonly onMessage: (data: unknown) => void,
    private readonly onFailure: (code: string) => void,
    private readonly isStopping: () => boolean,
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
      channel.send(JSON.stringify(command));
    } catch {
      this.onFailure("provider_command_failed");
    }
  }

  close(): void {
    this.channel?.close();
    this.channel = null;
    this.pending = [];
  }
}
