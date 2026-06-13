import type { RemoteAccessState, WebAccessSession, WebBranding } from "../../types";
import {
  apiForm,
  apiJson,
  clearPingReadRequest,
  clearWebAccessSessionReadRequest,
  RECENT_BOOTSTRAP_READ_TTL_MS,
  reuseRecentReadRequest,
  webAccessSessionRequestKey,
} from "./base";

export interface Observability {
  developer_mode?: boolean;
  log_level?: string;
  terminal_transcript?: {
    enabled?: boolean;
    per_actor_bytes?: number;
    persist?: boolean;
    strip_ansi?: boolean;
  };
  terminal_ui?: {
    scrollback_lines?: number;
  };
  runtime_visibility?: {
    peer_runtime?: "hidden" | "visible" | string;
    assistant_runtime?: "hidden" | "visible" | string;
  };
}

export interface AccessTokenEntry {
  token?: string;
  token_id?: string;
  token_preview?: string;
  user_id: string;
  is_admin: boolean;
  allowed_groups: string[];
  created_at: string;
}

export async function fetchObservability() {
  return apiJson<{ observability: Observability }>("/api/v1/observability");
}

export async function updateObservability(args: {
  developerMode: boolean;
  logLevel: "INFO" | "DEBUG";
  terminalTranscriptPerActorBytes?: number;
  terminalUiScrollbackLines?: number;
  peerRuntimeVisibility?: "hidden" | "visible";
  assistantRuntimeVisibility?: "hidden" | "visible";
}) {
  clearWebAccessSessionReadRequest();
  return apiJson<{ observability: Observability }>("/api/v1/observability", {
    method: "PUT",
    body: JSON.stringify({
      by: "user",
      developer_mode: args.developerMode,
      log_level: args.logLevel,
      terminal_transcript_per_actor_bytes: args.terminalTranscriptPerActorBytes,
      terminal_ui_scrollback_lines: args.terminalUiScrollbackLines,
      peer_runtime_visibility: args.peerRuntimeVisibility,
      assistant_runtime_visibility: args.assistantRuntimeVisibility,
    }),
  });
}

export async function fetchRemoteAccessState() {
  return apiJson<{ remote_access: RemoteAccessState }>("/api/v1/remote_access");
}

export async function fetchWebAccessSession() {
  return reuseRecentReadRequest(
    webAccessSessionRequestKey(),
    RECENT_BOOTSTRAP_READ_TTL_MS,
    () => apiJson<{ web_access_session: WebAccessSession }>("/api/v1/web_access/session"),
  );
}

export async function fetchWebBranding() {
  return apiJson<{ branding: WebBranding }>("/api/v1/branding");
}

export async function updateWebBranding(args: {
  productName?: string;
  clearLogoIcon?: boolean;
  clearFavicon?: boolean;
}) {
  return apiJson<{ branding: WebBranding }>("/api/v1/branding", {
    method: "PUT",
    body: JSON.stringify({
      by: "user",
      product_name: args.productName,
      clear_logo_icon: Boolean(args.clearLogoIcon),
      clear_favicon: Boolean(args.clearFavicon),
    }),
  });
}

export async function uploadWebBrandingAsset(assetKind: "logo_icon" | "favicon", file: File) {
  const form = new FormData();
  form.append("by", "user");
  form.append("file", file);
  return apiForm<{ branding: WebBranding }>(`/api/v1/branding/assets/${assetKind}`, form);
}

export async function clearWebBrandingAsset(assetKind: "logo_icon" | "favicon") {
  return apiJson<{ branding: WebBranding }>(`/api/v1/branding/assets/${assetKind}?by=user`, {
    method: "DELETE",
  });
}

export async function logoutWebAccess() {
  clearWebAccessSessionReadRequest();
  return apiJson<{ signed_out: boolean }>("/api/v1/web_access/logout", {
    method: "POST",
  });
}

export async function updateRemoteAccessConfig(args: {
  provider?: "off" | "manual" | "tailscale";
  mode?: string;
  enabled?: boolean;
  requireAccessToken?: boolean;
  webHost?: string;
  webPort?: number;
  webPublicUrl?: string;
}) {
  clearPingReadRequest();
  clearWebAccessSessionReadRequest();
  return apiJson<{ remote_access: RemoteAccessState }>("/api/v1/remote_access", {
    method: "PUT",
    body: JSON.stringify({
      by: "user",
      provider: args.provider,
      mode: args.mode,
      enabled: args.enabled,
      require_access_token: args.requireAccessToken,
      web_host: args.webHost,
      web_port: args.webPort,
      web_public_url: args.webPublicUrl,
    }),
  });
}

export async function startRemoteAccess() {
  return apiJson<{ remote_access: RemoteAccessState }>("/api/v1/remote_access/start?by=user", {
    method: "POST",
  });
}

export async function stopRemoteAccess() {
  return apiJson<{ remote_access: RemoteAccessState }>("/api/v1/remote_access/stop?by=user", {
    method: "POST",
  });
}

export async function applyRemoteAccess() {
  clearPingReadRequest();
  clearWebAccessSessionReadRequest();
  return apiJson<{
    accepted: boolean;
    remote_access: RemoteAccessState;
    target_local_url?: string | null;
    target_remote_url?: string | null;
  }>("/api/v1/remote_access/apply?by=user", {
    method: "POST",
  });
}

export async function fetchAccessTokens() {
  return apiJson<{ access_tokens: AccessTokenEntry[] }>("/api/v1/access-tokens");
}

export async function createAccessToken(
  userId: string,
  isAdmin: boolean,
  allowedGroups: string[],
  customToken?: string,
) {
  clearWebAccessSessionReadRequest();
  const body: Record<string, unknown> = {
    user_id: userId,
    is_admin: isAdmin,
    allowed_groups: allowedGroups,
  };
  if (customToken?.trim()) body.custom_token = customToken.trim();
  return apiJson<{ access_token: AccessTokenEntry }>("/api/v1/access-tokens", {
    method: "POST",
    body: JSON.stringify(body),
  });
}

export async function updateAccessToken(tokenId: string, updates: { allowed_groups?: string[]; is_admin?: boolean }) {
  clearWebAccessSessionReadRequest();
  return apiJson<{ access_token: AccessTokenEntry }>(`/api/v1/access-tokens/${encodeURIComponent(tokenId)}`, {
    method: "PATCH",
    body: JSON.stringify(updates),
  });
}

export async function revealAccessToken(tokenId: string) {
  return apiJson<{ token: string }>(`/api/v1/access-tokens/${encodeURIComponent(tokenId)}/reveal`);
}

export async function deleteAccessToken(tokenId: string) {
  clearWebAccessSessionReadRequest();
  return apiJson<{ deleted: boolean; access_tokens_remain?: boolean; deleted_current_session?: boolean }>(
    `/api/v1/access-tokens/${encodeURIComponent(tokenId)}`,
    { method: "DELETE" },
  );
}
