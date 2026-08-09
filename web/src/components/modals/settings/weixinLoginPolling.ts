import type { WeixinLoginStatus } from "../../../types";

const ACTIVE_LOGIN_STATUSES = new Set([
  "waiting_scan",
  "scanned",
  "scaned",
  "need_verify_code",
  "need_verifycode",
]);

type WeixinLoginPollingState = Pick<WeixinLoginStatus, "running" | "status">;

export function shouldPollWeixinLogin(status: WeixinLoginPollingState | null | undefined): boolean {
  if (status?.running === true) return true;
  return ACTIVE_LOGIN_STATUSES.has(
    String(status?.status || "")
      .trim()
      .toLowerCase(),
  );
}
