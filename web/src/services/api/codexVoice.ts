import { apiJson, withAuthToken } from "./base";

export type CodexVoiceCallInfo = {
  group_id: string;
  group_title: string;
  generation: string;
  analyst_generation: string;
  voice: string;
  connected: boolean;
};

export type CodexVoiceAnalystInfo = {
  group_id: string;
  group_title: string;
  generation: string;
  tui_ready: boolean;
  phase: "waiting" | "ready" | "working" | "needs_attention";
  last_result: string;
  warning: string;
};

export type CodexVoiceActiveResult = {
  call: CodexVoiceCallInfo | null;
  analyst: CodexVoiceAnalystInfo | null;
  voices: string[];
  default_voice: string;
  readiness: CodexVoiceReadiness;
};

export type CodexVoiceReadiness = {
  codex_cli_available: boolean;
  codex_credentials_available: boolean;
};

export type CodexVoiceStartResult = {
  call: CodexVoiceCallInfo;
  analyst: CodexVoiceAnalystInfo;
  answer_sdp: string;
  experimental: boolean;
};

export async function fetchActiveCodexVoiceCall() {
  return apiJson<CodexVoiceActiveResult>("/api/v1/codex_voice/calls/active");
}

export async function startCodexVoiceCall(
  groupId: string,
  args: { clientSessionId: string; offerSdp: string; voice: string; signal?: AbortSignal },
) {
  return apiJson<CodexVoiceStartResult>(
    `/api/v1/groups/${encodeURIComponent(groupId)}/codex_voice/calls`,
    {
      method: "POST",
      signal: args.signal,
      body: JSON.stringify({
        client_session_id: args.clientSessionId,
        offer_sdp: args.offerSdp,
        voice: args.voice,
      }),
    },
  );
}

export async function stopCodexVoiceCall(groupId: string, generation: string) {
  return apiJson<{ stopped: boolean }>(
    `/api/v1/groups/${encodeURIComponent(groupId)}/codex_voice/calls/${encodeURIComponent(generation)}`,
    { method: "DELETE" },
  );
}

export async function resetCodexVoiceAnalyst(groupId: string, generation: string) {
  return apiJson<{ analyst: CodexVoiceAnalystInfo }>(
    `/api/v1/groups/${encodeURIComponent(groupId)}/codex_voice/analysts/${encodeURIComponent(generation)}/reset`,
    { method: "POST" },
  );
}

export async function cancelCodexVoiceAnalyst(groupId: string, generation: string) {
  return apiJson<{ cancelled: boolean }>(
    `/api/v1/groups/${encodeURIComponent(groupId)}/codex_voice/analysts/${encodeURIComponent(generation)}/cancel`,
    { method: "POST" },
  );
}

export function getCodexVoiceWebSocketUrl(groupId: string, generation: string): string {
  const protocol = window.location.protocol === "https:" ? "wss:" : "ws:";
  return withAuthToken(
    `${protocol}//${window.location.host}/api/v1/groups/${encodeURIComponent(groupId)}/codex_voice/calls/${encodeURIComponent(generation)}/events`,
  );
}

export function getCodexVoiceTerminalWebSocketUrl(
  groupId: string,
  generation: string,
  query: string,
): string {
  const protocol = window.location.protocol === "https:" ? "wss:" : "ws:";
  const base = `${protocol}//${window.location.host}/api/v1/groups/${encodeURIComponent(groupId)}/codex_voice/analysts/${encodeURIComponent(generation)}/terminal`;
  return `${base}${query ? `?${query}` : ""}`;
}
