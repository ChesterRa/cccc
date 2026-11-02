import { describe, expect, it } from "vite-plus/test";

import type { LedgerEvent } from "../../types";
import { getMessageRowGrouping } from "./grouping";

function message(by: string, ts: string, data: Record<string, unknown> = {}): LedgerEvent {
  return {
    id: `${by}-${ts}`,
    kind: "chat.message",
    by,
    ts,
    data: { text: "Hello", to: ["user"], message_mode: "send", ...data },
  };
}

describe("message row grouping", () => {
  it("folds the head of a quick follow-up from the same sender", () => {
    const grouping = getMessageRowGrouping(
      message("claude", "2026-11-02T15:00:00Z"),
      message("claude", "2026-11-02T15:01:00Z"),
    );
    expect(grouping).toEqual({ collapseHeader: true, compactSpacing: true, sameSender: true });
  });

  it("keeps the head but marks the same sender after a pause, a reply or a request", () => {
    const first = message("claude", "2026-11-02T15:00:00Z");
    const later = message("claude", "2026-11-02T15:10:00Z");
    const reply = message("claude", "2026-11-02T15:01:00Z", { reply_to: "e0" });
    const request = message("claude", "2026-11-02T15:01:00Z", { message_mode: "request_reply" });
    for (const next of [later, reply, request]) {
      expect(getMessageRowGrouping(first, next)).toEqual({
        collapseHeader: false,
        compactSpacing: false,
        sameSender: true,
      });
    }
  });

  it("separates a change of sender, yours included", () => {
    const agent = message("claude", "2026-11-02T15:00:00Z");
    const user = message("user", "2026-11-02T15:00:30Z", { to: ["claude"] });
    expect(getMessageRowGrouping(agent, user).sameSender).toBe(false);
    expect(getMessageRowGrouping(user, agent).sameSender).toBe(false);
    expect(getMessageRowGrouping(user, { ...user, id: "u2" }).sameSender).toBe(true);
  });

  it("tells apart writers who share one relay sender", () => {
    const remote = (user: string, group = "g_remote") =>
      message("connect:i_remote", "2026-11-02T15:00:00Z", {
        source_platform: "cccc_connect",
        source_user_id: user,
        src_group_id: group,
      });
    expect(getMessageRowGrouping(remote("peer1"), remote("peer2")).sameSender).toBe(false);
    expect(getMessageRowGrouping(remote("peer1"), remote("peer1", "g_other")).sameSender).toBe(
      false,
    );
    expect(getMessageRowGrouping(remote("peer1"), remote("peer1"))).toEqual({
      collapseHeader: true,
      compactSpacing: true,
      sameSender: true,
    });

    const im = (user: string) =>
      message("user", "2026-11-02T15:00:00Z", {
        source_platform: "telegram",
        source_user_id: user,
      });
    expect(getMessageRowGrouping(im("staff-1"), im("staff-2")).sameSender).toBe(false);
    // An ID names its writer only on its own platform.
    const discord = message("user", "2026-11-02T15:00:00Z", {
      source_platform: "discord",
      source_user_id: "staff-1",
    });
    expect(getMessageRowGrouping(im("staff-1"), discord).sameSender).toBe(false);
    expect(getMessageRowGrouping(im("staff-1"), im("staff-1")).sameSender).toBe(true);
  });

  it("does not join a message to anything but a message", () => {
    const notice: LedgerEvent = { id: "n1", kind: "system.notify", by: "claude", data: {} };
    expect(
      getMessageRowGrouping(notice, message("claude", "2026-11-02T15:00:00Z")).sameSender,
    ).toBe(false);
    expect(getMessageRowGrouping(undefined, message("claude", "2026-11-02T15:00:00Z"))).toEqual({
      collapseHeader: false,
      compactSpacing: false,
      sameSender: false,
    });
  });
});
