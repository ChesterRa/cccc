import type { IMConfig, IMPlatform, IMStatus, WeixinLoginStatus } from "../../types";
import { apiJson } from "./base";

export interface IMAuthorizedChat {
  chat_id: string;
  thread_id: number | string;
  platform: string;
  authorized_at: number;
  key_used?: string;
  verbose?: boolean;
  authorization_source?: string;
}

export interface IMPendingRequest {
  key: string;
  chat_id: string;
  thread_id: number | string;
  platform: string;
  created_at: number;
  expires_at: number;
  expires_in_seconds: number;
}

export async function fetchIMStatus(groupId: string) {
  return apiJson<IMStatus>(`/api/im/status?group_id=${encodeURIComponent(groupId)}`);
}

export async function fetchIMConfig(groupId: string) {
  return apiJson<{ im: IMConfig | null }>(`/api/im/config?group_id=${encodeURIComponent(groupId)}`);
}

export async function setIMConfig(
  groupId: string,
  platform: IMPlatform,
  botTokenEnv: string,
  appTokenEnv?: string,
  extra?: {
    feishu_domain?: string;
    feishu_app_id?: string;
    feishu_app_secret?: string;
    dingtalk_app_key?: string;
    dingtalk_app_secret?: string;
    dingtalk_robot_code?: string;
    wecom_bot_id?: string;
    wecom_secret?: string;
    weixin_account_id?: string;
  },
) {
  const body: Record<string, unknown> = { group_id: groupId, platform };

  if (platform === "telegram" || platform === "slack" || platform === "discord") {
    body.bot_token_env = botTokenEnv;
    if (platform === "slack" && appTokenEnv) {
      body.app_token_env = appTokenEnv;
    }
  }

  if (platform === "feishu" && extra) {
    body.feishu_domain = extra.feishu_domain;
    body.feishu_app_id = extra.feishu_app_id;
    body.feishu_app_secret = extra.feishu_app_secret;
  }

  if (platform === "dingtalk" && extra) {
    body.dingtalk_app_key = extra.dingtalk_app_key;
    body.dingtalk_app_secret = extra.dingtalk_app_secret;
    body.dingtalk_robot_code = extra.dingtalk_robot_code;
  }

  if (platform === "wecom" && extra) {
    body.wecom_bot_id = extra.wecom_bot_id;
    body.wecom_secret = extra.wecom_secret;
  }

  if (platform === "weixin" && extra) {
    body.weixin_account_id = extra.weixin_account_id;
  }

  return apiJson("/api/im/set", { method: "POST", body: JSON.stringify(body) });
}

export async function unsetIMConfig(groupId: string) {
  return apiJson("/api/im/unset", { method: "POST", body: JSON.stringify({ group_id: groupId }) });
}

export async function startIMBridge(groupId: string) {
  return apiJson("/api/im/start", { method: "POST", body: JSON.stringify({ group_id: groupId }) });
}

export async function stopIMBridge(groupId: string) {
  return apiJson("/api/im/stop", { method: "POST", body: JSON.stringify({ group_id: groupId }) });
}

export async function fetchWeixinLoginStatus(groupId: string) {
  return apiJson<WeixinLoginStatus>(
    `/api/im/weixin/login/status?group_id=${encodeURIComponent(groupId)}`,
  );
}

export async function startWeixinLogin(groupId: string) {
  return apiJson<WeixinLoginStatus>("/api/im/weixin/login/start", {
    method: "POST",
    body: JSON.stringify({ group_id: groupId }),
  });
}

export async function verifyWeixinLogin(groupId: string, verifyCode: string) {
  return apiJson<WeixinLoginStatus>("/api/im/weixin/login/verify", {
    method: "POST",
    body: JSON.stringify({ group_id: groupId, verify_code: verifyCode }),
  });
}

export async function logoutWeixin(groupId: string) {
  return apiJson<WeixinLoginStatus>("/api/im/weixin/logout", {
    method: "POST",
    body: JSON.stringify({ group_id: groupId }),
  });
}

export async function fetchIMAuthorized(groupId: string) {
  return apiJson<{ authorized: IMAuthorizedChat[] }>(
    `/api/im/authorized?group_id=${encodeURIComponent(groupId)}`,
  );
}

export async function fetchIMPending(groupId: string) {
  return apiJson<{ pending: IMPendingRequest[] }>(
    `/api/im/pending?group_id=${encodeURIComponent(groupId)}`,
  );
}

export async function revokeIMChat(groupId: string, chatId: string, threadId: number | string = 0) {
  return apiJson<{ revoked: boolean; unsubscribed?: boolean }>(
    `/api/im/revoke?group_id=${encodeURIComponent(groupId)}&chat_id=${encodeURIComponent(chatId)}&thread_id=${encodeURIComponent(String(threadId))}`,
    { method: "POST" },
  );
}

export async function rejectIMPending(groupId: string, key: string) {
  return apiJson<{ rejected: boolean }>("/api/im/pending/reject", {
    method: "POST",
    body: JSON.stringify({ group_id: groupId, key }),
  });
}

export async function bindIMChat(groupId: string, key: string) {
  return apiJson<{ chat_id: string; thread_id: number | string; platform: string }>(
    "/api/im/bind",
    { method: "POST", body: JSON.stringify({ group_id: groupId, key }) },
  );
}

export async function setIMVerbose(
  groupId: string,
  chatId: string,
  verbose: boolean,
  threadId: number | string = 0,
) {
  return apiJson<{ chat_id: string; thread_id: number | string; verbose: boolean }>(
    `/api/im/verbose?group_id=${encodeURIComponent(groupId)}&chat_id=${encodeURIComponent(chatId)}&verbose=${verbose}&thread_id=${encodeURIComponent(String(threadId))}`,
    { method: "POST" },
  );
}
