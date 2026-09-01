import type { CodexVoicePhase } from "./codexVoiceTypes";

export class CodexVoicePeerMonitor {
  private disconnectTimer: number | null = null;

  bind(args: {
    peer: RTCPeerConnection;
    audio: HTMLAudioElement;
    resumeAudio(): Promise<boolean>;
    onPhase(phase: CodexVoicePhase): void;
    onFailure(code: string): void;
    isStopping(): boolean;
  }): void {
    const { peer, audio, resumeAudio, onPhase, onFailure, isStopping } = args;
    peer.ontrack = (event) => {
      audio.srcObject = event.streams[0] || new MediaStream([event.track]);
      void resumeAudio();
    };
    peer.onconnectionstatechange = () => {
      if (isStopping()) return;
      if (peer.connectionState === "connected") {
        this.clearDisconnectTimer();
        onPhase("listening");
        return;
      }
      if (peer.connectionState === "disconnected") {
        onPhase("connecting");
        this.clearDisconnectTimer();
        this.disconnectTimer = window.setTimeout(() => {
          if (peer.connectionState === "disconnected") onFailure("peer_disconnected");
        }, 5_000);
        return;
      }
      if (peer.connectionState === "failed" || peer.connectionState === "closed") {
        onFailure("peer_closed");
      }
    };
  }

  close(): void {
    this.clearDisconnectTimer();
  }

  private clearDisconnectTimer(): void {
    if (this.disconnectTimer !== null) window.clearTimeout(this.disconnectTimer);
    this.disconnectTimer = null;
  }
}
