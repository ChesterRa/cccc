import { apiJson, withAuthToken } from "./base";

export type VoiceNotificationScope = "off" | "to_user" | "all_chat";
export type VoicePreferences = {
  revision: number;
  groups: Record<string, VoiceNotificationScope>;
  suppress_viewed: boolean;
  verbosity: "concise" | "standard" | "detailed";
  style: "natural" | "direct" | "patient";
};
export type VoiceMessageRef = { group_id: string; event_id: string };
export type VoiceNotification = {
  sequence: number;
  source: VoiceMessageRef;
  kind: "request_reply" | "background";
  by: string;
  handoff: { analyst_generation: string; accepted: boolean } | null;
  processed: boolean;
  attempted: boolean;
  output_status: "processing" | "ready" | "unconfirmed" | "submitted" | "suppressed";
  suppression_reason: "viewed" | "policy" | "source_unavailable" | null;
};
export type VoiceNotificationSnapshot = {
  messages: VoiceNotification[];
  pending_count: number;
  unconfirmed_count: number;
  suppressed_count: number;
};
export const fetchVoicePreferences = () =>
  apiJson<{ preferences: VoicePreferences }>("/api/v1/codex_voice/preferences");
export const saveVoicePreferences = (preferences: VoicePreferences) =>
  apiJson<{ preferences: VoicePreferences }>("/api/v1/codex_voice/preferences", {
    method: "PUT",
    body: JSON.stringify({ preferences }),
  });
export const fetchVoiceNotifications = () =>
  apiJson<VoiceNotificationSnapshot>("/api/v1/codex_voice/notifications");
export const markVoiceMessagesViewed = (messages: VoiceMessageRef[]) =>
  apiJson<{ observed: number }>("/api/v1/codex_voice/messages/viewed", {
    method: "POST",
    body: JSON.stringify({ messages }),
  });

export type CodexVoiceCallInfo = {
  generation: string;
  analyst_generation: string;
  voice: string;
  connected: boolean;
};

export type CodexVoiceAnalystInfo = {
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
  analyst_runtime: string;
  analyst_runtime_available: boolean;
  realtime_credentials_available: boolean;
};

export type CodexVoiceStartResult = {
  call: CodexVoiceCallInfo;
  analyst: CodexVoiceAnalystInfo;
  answer_sdp: string;
  experimental: boolean;
};

export type CodexVoiceAnalystSettings = {
  runtime?: string;
  command?: string[];
  profile_id?: string;
  profile_scope?: "global" | "user";
  profile_owner?: string;
};

export type CodexVoiceAnalystSettingsResult = {
  settings: CodexVoiceAnalystSettings;
  environment_keys: string[];
};

export async function fetchActiveCodexVoiceCall() {
  return apiJson<CodexVoiceActiveResult>("/api/v1/codex_voice/calls/active");
}

export async function startCodexVoiceCall(args: {
  clientSessionId: string;
  offerSdp: string;
  voice: string;
  signal?: AbortSignal;
}) {
  return apiJson<CodexVoiceStartResult>("/api/v1/codex_voice/calls", {
    method: "POST",
    signal: args.signal,
    body: JSON.stringify({
      client_session_id: args.clientSessionId,
      offer_sdp: args.offerSdp,
      voice: args.voice,
    }),
  });
}

export async function stopCodexVoiceCall(generation: string) {
  return apiJson<{ stopped: boolean }>(
    `/api/v1/codex_voice/calls/${encodeURIComponent(generation)}`,
    { method: "DELETE" },
  );
}

export async function prepareCodexVoiceNotificationOutput(
  generation: string,
  resultId: string,
  signal?: AbortSignal,
) {
  return apiJson<{ message: unknown | null }>(
    `/api/v1/codex_voice/calls/${encodeURIComponent(generation)}/notification-output`,
    { method: "POST", body: JSON.stringify({ result_id: resultId }), signal },
  );
}

export async function resetCodexVoiceAnalyst(generation: string) {
  return apiJson<{ analyst: CodexVoiceAnalystInfo }>(
    `/api/v1/codex_voice/analysts/${encodeURIComponent(generation)}/reset`,
    { method: "POST" },
  );
}

export async function cancelCodexVoiceAnalyst(generation: string) {
  return apiJson<{ cancelled: boolean }>(
    `/api/v1/codex_voice/analysts/${encodeURIComponent(generation)}/cancel`,
    { method: "POST" },
  );
}

export function getCodexVoiceWebSocketUrl(generation: string): string {
  const protocol = window.location.protocol === "https:" ? "wss:" : "ws:";
  return withAuthToken(
    `${protocol}//${window.location.host}/api/v1/codex_voice/calls/${encodeURIComponent(generation)}/events`,
  );
}

export function getCodexVoiceTerminalWebSocketUrl(generation: string, query: string): string {
  const protocol = window.location.protocol === "https:" ? "wss:" : "ws:";
  const base = `${protocol}//${window.location.host}/api/v1/codex_voice/analysts/${encodeURIComponent(generation)}/terminal`;
  return `${base}${query ? `?${query}` : ""}`;
}

export async function fetchCodexVoiceAnalystSettings() {
  return apiJson<CodexVoiceAnalystSettingsResult>("/api/v1/codex_voice/analyst-settings");
}

export async function updateCodexVoiceAnalystSettings(args: {
  settings: {
    runtime: string;
    command: string;
    profile_id: string;
    profile_scope: "global" | "user";
    profile_owner: string;
  };
  environmentSet: Record<string, string>;
  environmentUnset: string[];
  environmentClear: boolean;
  discardCurrentWork: boolean;
}) {
  return apiJson<{
    analyst: CodexVoiceAnalystInfo | null;
    restarted: boolean;
    started_new_session: boolean;
    discarded_work: boolean;
  }>("/api/v1/codex_voice/analyst-settings", {
    method: "PUT",
    body: JSON.stringify({
      settings: args.settings,
      environment_set: args.environmentSet,
      environment_unset: args.environmentUnset,
      environment_clear: args.environmentClear,
      discard_current_work: args.discardCurrentWork,
    }),
  });
}
