import { describe, expect, it } from "vite-plus/test";

import type { WeixinLoginStatus } from "../../../types";
import { shouldPollWeixinLogin } from "./weixinLoginPolling";

function status(value: string, running = false): WeixinLoginStatus {
  return { status: value, logged_in: false, running };
}

describe("Weixin login polling", () => {
  it.each(["waiting_scan", "scanned", "need_verify_code"])(
    "continues polling the intermediate QR state %s",
    (value) => {
      expect(shouldPollWeixinLogin(status(value))).toBe(true);
    },
  );

  it("continues polling while the native login worker is running", () => {
    expect(shouldPollWeixinLogin(status("starting", true))).toBe(true);
  });

  it.each(["logged_in", "expired", "error", "logged_out"])(
    "stops polling the terminal state %s",
    (value) => {
      expect(shouldPollWeixinLogin(status(value))).toBe(false);
    },
  );
});
