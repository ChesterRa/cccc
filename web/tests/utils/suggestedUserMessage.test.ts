import { beforeEach, describe, expect, it, vi } from "vitest";

import type { LedgerEvent } from "../../src/types";
import {
  SUGGESTED_USER_MESSAGE_CONSUMED_KEY,
  SUGGESTED_USER_MESSAGE_MAX_CHARS,
  composerTargetAllowsSuggestedUserMessage,
  consumeSuggestedUserMessage,
  latestSuggestedUserMessage,
  normalizeSuggestedUserMessage,
  readConsumedSuggestedUserMessageIds,
} from "../../src/utils/suggestedUserMessage";

function chatMessage(
  id: string,
  by: string,
  text: string,
  extra: Record<string, unknown> = {},
): LedgerEvent {
  return {
    id,
    kind: "chat.message",
    by,
    ts: `2026-06-14T00:00:${id.slice(-1) || "0"}Z`,
    data: {
      text,
      to: ["user"],
      ...extra,
    },
  };
}

function makeStorage(options: { throwOnSet?: boolean } = {}) {
  const store = new Map<string, string>();
  return {
    getItem: vi.fn((key: string) => store.get(key) ?? null),
    setItem: vi.fn((key: string, value: string) => {
      if (options.throwOnSet) throw new Error("storage unavailable");
      store.set(key, value);
    }),
    clear: () => store.clear(),
  };
}

describe("suggestedUserMessage", () => {
  beforeEach(() => {
    vi.unstubAllGlobals();
  });

  it("normalizes whitespace and caps stored suggestion length", () => {
    const longText = `  ${"x".repeat(SUGGESTED_USER_MESSAGE_MAX_CHARS + 10)}  `;

    const normalized = normalizeSuggestedUserMessage(longText);

    expect(normalized).toHaveLength(SUGGESTED_USER_MESSAGE_MAX_CHARS);
    expect(normalized).toBe("x".repeat(SUGGESTED_USER_MESSAGE_MAX_CHARS));
  });

  it("returns the latest unconsumed suggestion from a non-user message addressed to user", () => {
    const suggestion = latestSuggestedUserMessage([
      chatMessage("evt-1", "peer1", "Done.", { suggested_user_message: "Try the next step." }),
    ]);

    expect(suggestion).toMatchObject({
      eventId: "evt-1",
      by: "peer1",
      text: "Try the next step.",
    });
  });

  it("does not resurrect older suggestions after a newer user-addressed chat message", () => {
    const suggestion = latestSuggestedUserMessage([
      chatMessage("evt-1", "peer1", "Done.", { suggested_user_message: "Try the next step." }),
      chatMessage("evt-2", "peer2", "Separate update."),
    ]);

    expect(suggestion).toBeNull();
  });

  it("ignores later peer-only messages when looking for the current user suggestion", () => {
    const suggestion = latestSuggestedUserMessage([
      chatMessage("evt-1", "peer1", "Done.", { suggested_user_message: "Try the next step." }),
      chatMessage("evt-2", "peer2", "Peer-only update.", { to: ["peer1"] }),
    ]);

    expect(suggestion?.eventId).toBe("evt-1");
    expect(suggestion?.text).toBe("Try the next step.");
  });

  it("does not show suggestions after the user has already sent a later message", () => {
    const suggestion = latestSuggestedUserMessage([
      chatMessage("evt-1", "peer1", "Done.", { suggested_user_message: "Try the next step." }),
      chatMessage("evt-2", "user", "I already replied."),
    ]);

    expect(suggestion).toBeNull();
  });

  it("does not show suggestions from messages that are not addressed to user", () => {
    const suggestion = latestSuggestedUserMessage([
      chatMessage("evt-1", "peer1", "Peer update.", {
        to: ["peer2"],
        suggested_user_message: "This should not prefill the user composer.",
      }),
    ]);

    expect(suggestion).toBeNull();
  });

  it("persists consumed suggestion ids and hides consumed suggestions", () => {
    const localStorage = makeStorage();
    vi.stubGlobal("window", { localStorage });

    consumeSuggestedUserMessage("evt-1");

    expect(localStorage.setItem).toHaveBeenCalledWith(
      SUGGESTED_USER_MESSAGE_CONSUMED_KEY,
      JSON.stringify(["evt-1"]),
    );
    expect(readConsumedSuggestedUserMessageIds()).toEqual(new Set(["evt-1"]));
    expect(latestSuggestedUserMessage([
      chatMessage("evt-1", "peer1", "Done.", { suggested_user_message: "Try the next step." }),
    ], readConsumedSuggestedUserMessageIds())).toBeNull();
  });

  it("does not throw when localStorage cannot persist dismissal", () => {
    vi.stubGlobal("window", { localStorage: makeStorage({ throwOnSet: true }) });

    expect(() => consumeSuggestedUserMessage("evt-1")).not.toThrow();
  });

  it("only allows composer suggestions for the selected group target", () => {
    expect(composerTargetAllowsSuggestedUserMessage({
      selectedGroupId: "g-current",
      destGroupId: "g-current",
      composerGroupSettled: true,
    })).toBe(true);

    expect(composerTargetAllowsSuggestedUserMessage({
      selectedGroupId: "g-current",
      destGroupId: "g-other",
      composerGroupSettled: true,
    })).toBe(false);

    expect(composerTargetAllowsSuggestedUserMessage({
      selectedGroupId: "g-current",
      destGroupId: "g-current",
      composerGroupSettled: false,
    })).toBe(false);
  });
});
