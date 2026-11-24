import { afterEach, expect, it } from "vite-plus/test";

import i18n, { i18nReady } from "../i18n";
import { formatSendMessageError } from "./chatSend";

afterEach(async () => {
  await i18nReady;
  await i18n.changeLanguage("en");
});

it("distinguishes an unconfirmed response from a confirmed rejection", () => {
  for (const code of ["NETWORK_ERROR", "EMPTY_RESPONSE", "PARSE_ERROR"]) {
    expect(formatSendMessageError({ code, message: "fetch failed", t: (key) => key })).toBe(
      "sendResultUnknown",
    );
  }
  expect(
    formatSendMessageError({ code: "permission_denied", message: "Not allowed", t: (key) => key }),
  ).toBe("permission_denied: Not allowed");
});

it("localizes attachment-size rejection from client or server", () => {
  for (const message of [
    "Attachments exceed 100 MiB in total.",
    "combined files exceed the message attachment limit",
    "Connect attachments exceed 10 MiB in total",
  ]) {
    expect(formatSendMessageError({ code: "attachment_too_large", message, t: (key) => key })).toBe(
      "attachmentsTooLarge",
    );
  }
});

it("routes mapped daemon errors through formatDaemonError while preserving original codes", async () => {
  await i18nReady;
  await i18n.changeLanguage("zh");
  const formatted = formatSendMessageError({
    code: "no_enabled_recipients",
    message: "No recipients remain after excluding the sender",
    t: (key) => key,
  });
  expect(formatted).not.toContain("No recipients remain");
  expect(formatted).toContain("（no_enabled_recipients）");
});
