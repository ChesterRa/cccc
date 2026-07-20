import { describe, expect, it } from "vite-plus/test";

import type { LedgerEvent } from "../../types";
import {
  buildComposerHistoryEntries,
  canStartComposerHistory,
  getComposerHistoryText,
  moveComposerHistory,
  startComposerHistory,
} from "./chatComposerHistory";

function message(
  text: unknown,
  options: {
    by?: string;
    kind?: string;
    optimistic?: boolean;
    srcGroupId?: string;
    streaming?: boolean;
  } = {},
): LedgerEvent {
  return {
    id: `ev_${String(text)}`,
    ts: "2026-07-17T00:00:00Z",
    kind: options.kind || "chat.message",
    group_id: "g_local",
    by: options.by || "user",
    _streaming: options.streaming,
    data: { text, _optimistic: options.optimistic, src_group_id: options.srcGroupId },
  } as LedgerEvent;
}

describe("composer message history entries", () => {
  it("keeps canonical local user text in ledger order", () => {
    expect(
      buildComposerHistoryEntries([
        message("first"),
        message("agent reply", { by: "peer1" }),
        message("pending", { optimistic: true }),
        message("projected from another group", { srcGroupId: "g_source" }),
        message("streaming", { streaming: true }),
        message("not chat", { kind: "system.notify" }),
        message("  \n"),
        message("second\nline"),
      ]),
    ).toEqual(["first", "second\nline"]);
  });

  it("preserves repeated messages and exact canonical text", () => {
    expect(buildComposerHistoryEntries([message(" same "), message(" same ")])).toEqual([
      " same ",
      " same ",
    ]);
  });
});

describe("composer message history activation", () => {
  const eligible = {
    composerText: "",
    composerGroupSettled: true,
    selectedGroupId: "g_local",
    busy: "",
    menuOpen: false,
    isComposing: false,
    hasModifier: false,
  };

  it("starts only from an exactly empty, settled composer", () => {
    expect(canStartComposerHistory(eligible)).toBe(true);
    expect(canStartComposerHistory({ ...eligible, composerText: " " })).toBe(false);
    expect(canStartComposerHistory({ ...eligible, composerGroupSettled: false })).toBe(false);
    expect(canStartComposerHistory({ ...eligible, selectedGroupId: "" })).toBe(false);
  });

  it("does not compete with sends, menus, IME, or modified arrows", () => {
    expect(canStartComposerHistory({ ...eligible, busy: "send" })).toBe(false);
    expect(canStartComposerHistory({ ...eligible, menuOpen: true })).toBe(false);
    expect(canStartComposerHistory({ ...eligible, isComposing: true })).toBe(false);
    expect(canStartComposerHistory({ ...eligible, hasModifier: true })).toBe(false);
  });
});

describe("composer message history navigation", () => {
  it("starts at the newest entry and stops at the oldest entry", () => {
    const started = startComposerHistory(["first", "second", "third"], "g_local", "");
    expect(started).not.toBeNull();
    expect(getComposerHistoryText(started!)).toBe("third");

    const second = moveComposerHistory(started!, "older");
    expect(second.text).toBe("second");
    const first = moveComposerHistory(second.session!, "older");
    expect(first.text).toBe("first");
    const stillFirst = moveComposerHistory(first.session!, "older");
    expect(stillFirst.text).toBe("first");
  });

  it("moves newer and restores the original draft after the newest entry", () => {
    const started = startComposerHistory(["first", "second"], "g_local", "original draft")!;
    const first = moveComposerHistory(started, "older");
    const second = moveComposerHistory(first.session!, "newer");
    expect(second.text).toBe("second");
    expect(second.session).not.toBeNull();

    const restored = moveComposerHistory(second.session!, "newer");
    expect(restored).toEqual({ session: null, text: "original draft" });
  });

  it("does not start without history entries", () => {
    expect(startComposerHistory([], "g_local", "draft")).toBeNull();
  });
});
