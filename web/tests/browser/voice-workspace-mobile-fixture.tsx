// Real voice workspace; HTTP, device enumeration and ASR transport are fixture boundaries.
import { useEffect, useState } from "react";
import i18next from "../../src/i18n";
import { useUIStore } from "../../src/stores/useUIStore";
import {
  VoiceSecretaryComposerControl,
  type VoiceSecretaryCaptureMode,
} from "../../src/pages/chat/VoiceSecretaryComposerControl";
import "../../src/index.css";

await i18next.changeLanguage("zh");
const params = new URLSearchParams(location.search);
const documentFixture = {
  doc_id: "fixture-document",
  title: params.has("long") ? "跨团队语音会议记录与后续事项跟进工作文档" : "语音会议工作稿",
  workspace_path: params.has("long")
    ? "voice/meetings/long-project-name-without-spaces/9e141dcf216f49d7.md"
    : "voice/9e141dcf216f49d7.md",
  status: "active",
  content:
    "# 会议记录与工作计划\n\n## 当前进展\n\n" +
    "本次讨论确认需求、负责人与交付时间。记录应清楚呈现，方便手机阅读和后续跟进。\n\n".repeat(16),
};
const assistant = {
  assistant_id: "voice_secretary",
  enabled: true,
  lifecycle: "ready",
  config: { recognition_backend: "assistant_service_local_asr", recognition_language: "mixed" },
  health: {
    actor: { running: true },
    service: { ready: true, streaming_backend: { ready: true } },
  },
};
const probe = {
  enumerations: 0,
  starts: 0,
  stops: 0,
  languages: [] as string[],
  devices: [] as string[],
};
Object.assign(window, { voiceWorkspaceProbe: probe });
const replies = [
  {
    request_id: "first",
    status: "done",
    reply_text: "语音已识别完成，整理后的提示词可以直接使用。",
    created_at: "2026-01-02T08:00:00Z",
    updated_at: "2026-01-02T08:00:00Z",
  },
  {
    request_id: "last",
    status: "needs_user",
    reply_text: "只识别到“一系”，信息不足，请重新说一遍完整问题。",
    created_at: "2026-01-01T08:00:00Z",
    updated_at: "2026-01-01T08:00:00Z",
  },
];
const extraRows = Number(new URLSearchParams(location.search).get("extra") || 0);
for (let i = 0; i < extraRows; i++)
  replies.push({
    request_id: `older-${i}`,
    status: "done",
    reply_text: `更早的识别记录 ${i + 1}，用于验证动态区独立滚动。`,
    created_at: "2025-12-01T08:00:00Z",
    updated_at: "2025-12-01T08:00:00Z",
  });
window.fetch = async (input, options) => {
  const url = typeof input === "string" ? input : input instanceof URL ? input.href : input.url;
  const body = options?.body ? JSON.parse(String(options.body)) : {};
  if (url.endsWith("/settings") && options?.method === "PUT") {
    Object.assign(assistant.config, body.config);
    if (typeof body.enabled === "boolean") assistant.enabled = body.enabled;
    if (body.config?.recognition_language) probe.languages.push(body.config.recognition_language);
  }
  if (url.includes("recording_lease"))
    return Response.json({ ok: true, result: { lease_id: "fixture", lost: false } });
  return Response.json({
    ok: true,
    result: { assistant, documents: [documentFixture], sessions: [], ask_requests: replies },
  });
};
navigator.mediaDevices.enumerateDevices = async () => {
  probe.enumerations++;
  return [
    {
      deviceId: "fixture-mic",
      groupId: "fixture",
      kind: "audioinput",
      label: "测试麦克风",
      toJSON: () => ({}),
    } as MediaDeviceInfo,
  ];
};
let deviceContext: AudioContext | undefined;
navigator.mediaDevices.getUserMedia = async (constraints) => {
  const audio = constraints?.audio;
  if (audio && typeof audio === "object")
    probe.devices.push(JSON.stringify(audio.deviceId || "default"));
  deviceContext ??= new AudioContext();
  return deviceContext.createMediaStreamDestination().stream;
};
class AsrSocket {
  static OPEN = 1;
  static CONNECTING = 0;
  static CLOSING = 2;
  static CLOSED = 3;
  readyState = 0;
  bufferedAmount = 0;
  binaryType = "arraybuffer";
  onopen: (() => void) | null = null;
  onmessage: ((event: { data: string }) => void) | null = null;
  onclose: ((event: { code: number; reason: string }) => void) | null = null;
  constructor() {
    setTimeout(() => {
      this.readyState = 1;
      this.onopen?.();
    }, 20);
  }
  send(data: unknown) {
    if (typeof data !== "string") return;
    const message = JSON.parse(data);
    if (message.type === "start") {
      probe.starts++;
      this.onmessage?.({ data: JSON.stringify({ type: "ready" }) });
    }
    if (message.type === "stop") {
      probe.stops++;
      this.onmessage?.({ data: JSON.stringify({ type: "closed", ok: true }) });
    }
  }
  close() {
    this.readyState = 3;
    this.onclose?.({ code: 1000, reason: "" });
  }
}
Object.assign(window, { WebSocket: AsrSocket });
export function Fixture() {
  const [mode, setMode] = useState<VoiceSecretaryCaptureMode>(
    (params.get("mode") || "prompt") as VoiceSecretaryCaptureMode,
  );
  const dark = params.get("theme") !== "light";
  const scale = params.get("scale") || "100";
  useEffect(() => {
    document.documentElement.classList.toggle("dark", dark);
    document.documentElement.classList.toggle("light", !dark);
    document.documentElement.style.fontSize = `${scale}%`;
    const update = () => useUIStore.getState().setSmallScreen(innerWidth < 768);
    update();
    window.addEventListener("resize", update);
    return () => window.removeEventListener("resize", update);
  }, [dark, scale]);
  return (
    <main>
      <div aria-label="Background messages" style={{ height: "100dvh", overflowY: "auto" }}>
        <div style={{ height: "2000px" }}>聊天列表</div>
      </div>
      <VoiceSecretaryComposerControl
        isDark={dark}
        selectedGroupId="workspace-fixture"
        busy=""
        initiallyOpen
        captureMode={mode}
        onCaptureModeChange={setMode}
        composerText="测试草稿"
      />
    </main>
  );
}
