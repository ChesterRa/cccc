import type { ApiResponse } from "../../../services/api";

type RevokeResponse = ApiResponse<{ revoked: boolean }>;

export function imRevokeKey(chatId: string, threadId: number | string): string {
  return `${chatId}:${threadId}`;
}

export async function revokeIMChatAuthorization({
  request,
  refresh,
  fallbackError,
}: {
  request: () => Promise<RevokeResponse>;
  refresh: () => Promise<void>;
  fallbackError: string;
}): Promise<string | null> {
  try {
    const response = await request();
    if (!response.ok) return response.error.message || fallbackError;
    if (response.result.revoked !== true) return fallbackError;
    await refresh();
    return null;
  } catch {
    return fallbackError;
  }
}
