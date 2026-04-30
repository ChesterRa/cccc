import type { PresentationBrowserSurfaceState } from "../../types";
import type { ApiResponse } from "./base";
import {
  apiJson,
  normalizePresentationBrowserSurfaceState,
  withAuthToken,
} from "./base";

export type WebModelConnector = {
  connector_id: string;
  kind?: string;
  group_id: string;
  actor_id: string;
  provider?: string;
  label?: string;
  secret_preview?: string;
  revoked?: boolean;
  created_at?: string;
  updated_at?: string;
  last_activity_at?: string;
  last_method?: string;
  last_tool_name?: string;
  last_call_status?: string;
  last_wait_status?: string;
  last_turn_id?: string;
  last_error?: string;
  connector_url?: string;
  connector_url_with_token?: string;
  secret_available?: boolean;
};

export type WebModelConnectorCreateResult = {
  connector: WebModelConnector;
  secret: string;
  replaced_connector_ids?: string[];
};

export type WebModelBrowserSession = {
  active?: boolean;
  ready?: boolean;
  login_required?: boolean;
  pid?: number;
  cdp_port?: number;
  profile_dir?: string;
  visibility?: string;
  tab_url?: string;
  last_tab_url?: string;
  conversation_url?: string;
  bootstrap_seed_delivered_at?: string;
  auto_confirm_scan_at?: string;
  auto_confirm_pages_seen?: number;
  auto_confirm_candidate_count?: number;
  auto_confirm_last_at?: string;
  auto_confirm_last_count?: number;
  auto_confirm_total?: number;
  auto_confirm_last_page_url?: string;
  auto_confirm_last_details?: Array<Record<string, unknown>>;
  auto_confirm_last_errors?: Array<Record<string, unknown>>;
  last_delivery_at?: string;
  last_turn_id?: string;
  error?: string;
  message?: string;
};

export type WebModelBrowserSurfaceResult = {
  browser_session: WebModelBrowserSession;
  browser_surface: PresentationBrowserSurfaceState;
};

export async function fetchWebModelConnectors() {
  return apiJson<{ connectors: WebModelConnector[] }>("/api/v1/web-model/connectors");
}

export async function createWebModelConnector(args: {
  groupId: string;
  actorId: string;
  provider?: string;
  label?: string;
}) {
  return apiJson<WebModelConnectorCreateResult>("/api/v1/web-model/connectors", {
    method: "POST",
    body: JSON.stringify({
      group_id: String(args.groupId || "").trim(),
      actor_id: String(args.actorId || "").trim(),
      provider: String(args.provider || "").trim(),
      label: String(args.label || "").trim(),
    }),
  });
}

export async function revokeWebModelConnector(connectorId: string) {
  return apiJson<{ revoked: boolean; connector_id: string }>(
    `/api/v1/web-model/connectors/${encodeURIComponent(String(connectorId || "").trim())}`,
    { method: "DELETE" },
  );
}

export async function fetchWebModelBrowserSession(groupId: string, actorId: string) {
  const params = new URLSearchParams({
    group_id: String(groupId || "").trim(),
    actor_id: String(actorId || "").trim(),
  });
  return apiJson<{ browser_session: WebModelBrowserSession }>(`/api/v1/web-model/browser-session?${params.toString()}`);
}

export async function fetchWebModelBrowserSurfaceSession(
  groupId: string,
  actorId: string,
): Promise<ApiResponse<WebModelBrowserSurfaceResult>> {
  const params = new URLSearchParams({
    group_id: String(groupId || "").trim(),
    actor_id: String(actorId || "").trim(),
  });
  const resp = await apiJson<WebModelBrowserSurfaceResult>(`/api/v1/web-model/browser-session?${params.toString()}`);
  if (!resp.ok) return resp;
  return {
    ok: true,
    result: {
      browser_session: resp.result.browser_session || {},
      browser_surface: normalizePresentationBrowserSurfaceState(resp.result.browser_surface),
    },
  };
}

export async function openWebModelBrowserSession(args: {
  groupId: string;
  actorId: string;
  visibility?: "visible" | "background" | "headless" | string;
}) {
  return apiJson<{ browser_session: WebModelBrowserSession }>("/api/v1/web-model/browser-session/open", {
    method: "POST",
    body: JSON.stringify({
      group_id: String(args.groupId || "").trim(),
      actor_id: String(args.actorId || "").trim(),
      visibility: String(args.visibility || "visible").trim() || "visible",
    }),
  });
}

export async function openWebModelBrowserSurfaceSession(args: {
  groupId: string;
  actorId: string;
  width?: number;
  height?: number;
}): Promise<ApiResponse<WebModelBrowserSurfaceResult>> {
  const resp = await apiJson<WebModelBrowserSurfaceResult>("/api/v1/web-model/browser-session/open", {
    method: "POST",
    body: JSON.stringify({
      group_id: String(args.groupId || "").trim(),
      actor_id: String(args.actorId || "").trim(),
      width: Math.max(640, Math.min(2560, Math.round(Number(args.width || 1366)))),
      height: Math.max(480, Math.min(1600, Math.round(Number(args.height || 900)))),
    }),
  });
  if (!resp.ok) return resp;
  return {
    ok: true,
    result: {
      browser_session: resp.result.browser_session || {},
      browser_surface: normalizePresentationBrowserSurfaceState(resp.result.browser_surface),
    },
  };
}

export async function closeWebModelBrowserSession(groupId: string, actorId: string) {
  return apiJson<{ browser_session: WebModelBrowserSession }>("/api/v1/web-model/browser-session/close", {
    method: "POST",
    body: JSON.stringify({
      group_id: String(groupId || "").trim(),
      actor_id: String(actorId || "").trim(),
    }),
  });
}

export async function closeWebModelBrowserSurfaceSession(
  groupId: string,
  actorId: string,
): Promise<ApiResponse<WebModelBrowserSurfaceResult>> {
  const resp = await apiJson<WebModelBrowserSurfaceResult>("/api/v1/web-model/browser-session/close", {
    method: "POST",
    body: JSON.stringify({
      group_id: String(groupId || "").trim(),
      actor_id: String(actorId || "").trim(),
    }),
  });
  if (!resp.ok) return resp;
  return {
    ok: true,
    result: {
      browser_session: resp.result.browser_session || {},
      browser_surface: normalizePresentationBrowserSurfaceState(resp.result.browser_surface),
    },
  };
}

export async function bindCurrentWebModelBrowserConversation(args: {
  groupId: string;
  actorId: string;
  conversationUrl?: string;
  clear?: boolean;
}): Promise<ApiResponse<WebModelBrowserSurfaceResult>> {
  const resp = await apiJson<WebModelBrowserSurfaceResult>("/api/v1/web-model/browser-session/bind-current", {
    method: "POST",
    body: JSON.stringify({
      group_id: String(args.groupId || "").trim(),
      actor_id: String(args.actorId || "").trim(),
      conversation_url: String(args.conversationUrl || "").trim(),
      clear: Boolean(args.clear),
    }),
  });
  if (!resp.ok) return resp;
  return {
    ok: true,
    result: {
      browser_session: resp.result.browser_session || {},
      browser_surface: normalizePresentationBrowserSurfaceState(resp.result.browser_surface),
    },
  };
}

export function getWebModelBrowserSurfaceWebSocketUrl(groupId: string, actorId: string): string {
  const protocol = window.location.protocol === "https:" ? "wss:" : "ws:";
  const params = new URLSearchParams({
    group_id: String(groupId || "").trim(),
    actor_id: String(actorId || "").trim(),
  });
  return withAuthToken(`${protocol}//${window.location.host}/api/v1/web-model/browser-session/ws?${params.toString()}`);
}
