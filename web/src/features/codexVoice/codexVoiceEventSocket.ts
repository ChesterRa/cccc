import { getCodexVoiceWebSocketUrl, type CodexVoiceCallInfo } from "../../services/api";
import { eventStreamCloseCode, failure, normalizedErrorCode } from "./codexVoiceProtocol";

const SERVER_READY_TIMEOUT_MS = 10_000;
const SERVER_HEARTBEAT_INTERVAL_MS = 15_000;

export type CodexVoiceServerMessage = Record<string, unknown>;

export class CodexVoiceEventSocket {
  private socket: WebSocket | null = null;
  private heartbeatTimer: number | null = null;
  private lastServerErrorCode = "";

  constructor(
    private readonly call: CodexVoiceCallInfo,
    private readonly onMessage: (message: CodexVoiceServerMessage) => void,
    private readonly onFailure: (code: string) => void,
    private readonly isStopping: () => boolean,
  ) {}

  connect(): Promise<void> {
    const socket = new WebSocket(
      getCodexVoiceWebSocketUrl(this.call.group_id, this.call.generation),
    );
    this.socket = socket;
    return new Promise<void>((resolve, reject) => {
      let ready = false;
      const timeout = window.setTimeout(() => {
        if (!ready) reject(failure("event_stream_timeout"));
      }, SERVER_READY_TIMEOUT_MS);
      socket.onmessage = (event) => {
        let message: CodexVoiceServerMessage;
        try {
          message = JSON.parse(String(event.data)) as CodexVoiceServerMessage;
        } catch {
          this.onFailure("invalid_server_event");
          return;
        }
        if (message.type === "ready") {
          ready = true;
          window.clearTimeout(timeout);
          this.heartbeatTimer = window.setInterval(() => {
            this.send({ type: "heartbeat" });
          }, SERVER_HEARTBEAT_INTERVAL_MS);
          resolve();
          return;
        }
        if (message.type === "error") {
          this.lastServerErrorCode = normalizedErrorCode(message.code) || "unknown";
        } else {
          this.lastServerErrorCode = "";
        }
        this.onMessage(message);
      };
      socket.onerror = () => {
        if (!ready) {
          window.clearTimeout(timeout);
          reject(failure("event_stream_connect_failed"));
        }
      };
      socket.onclose = () => {
        window.clearTimeout(timeout);
        this.clearHeartbeat();
        if (!ready) reject(failure("event_stream_setup_closed"));
        else if (!this.isStopping()) {
          this.onFailure(eventStreamCloseCode(this.lastServerErrorCode));
        }
      };
    });
  }

  send(message: unknown): boolean {
    const socket = this.socket;
    if (!socket || socket.readyState !== WebSocket.OPEN) return false;
    try {
      socket.send(JSON.stringify(message));
      return true;
    } catch {
      return false;
    }
  }

  close(): void {
    this.clearHeartbeat();
    this.socket?.close(1000, "voice session stopped");
    this.socket = null;
  }

  private clearHeartbeat(): void {
    if (this.heartbeatTimer !== null) window.clearInterval(this.heartbeatTimer);
    this.heartbeatTimer = null;
  }
}
