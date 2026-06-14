import { describe, expect, it } from "vitest";

import {
  defaultTargetDraftFromSession,
  liveBrowserConversationUrlFromSession,
  savedTargetDraftFromSession,
  targetDraftMatchesSaved,
} from "../../../../src/utils/webModelTargetDraft";

describe("ChatGPT Web Model target draft model", () => {
  it("keeps a saved conversation URL as the existing-chat draft", () => {
    const session = {
      conversation_url: "https://chatgpt.com/c/saved-chat",
      delivery_target: {
        state: "bound_existing_chat",
        url: "https://chatgpt.com/c/saved-chat",
      },
    };

    expect(savedTargetDraftFromSession(session).mode).toBe("existing");
    expect(defaultTargetDraftFromSession(session, "https://chatgpt.com/c/current-chat")).toEqual({
      mode: "existing",
      url: "https://chatgpt.com/c/saved-chat",
    });
  });

  it("uses the saved target URL from the health snapshot when the top-level session is incomplete", () => {
    const session = {
      delivery_target: {
        state: "none",
      },
      health_snapshot: {
        target: {
          state: "bound_existing_chat",
          url: "https://chatgpt.com/c/health-saved-chat",
        },
      },
    };

    expect(defaultTargetDraftFromSession(session, "https://chatgpt.com/c/current-chat")).toEqual({
      mode: "existing",
      url: "https://chatgpt.com/c/health-saved-chat",
    });
  });

  it("keeps Save disabled when the existing-chat URL already matches the saved chat", () => {
    const saved = {
      mode: "existing" as const,
      url: "https://chatgpt.com/c/saved-chat",
    };

    expect(targetDraftMatchesSaved({
      mode: "existing",
      url: "https://chatgpt.com/c/saved-chat",
      saved,
    })).toBe(true);
    expect(targetDraftMatchesSaved({
      mode: "existing",
      url: "https://chatgpt.com/c/other-chat",
      saved,
    })).toBe(false);
  });

  it("enables first-time save when no target is saved and the current tab is a ChatGPT chat", () => {
    const session = {
      delivery_target: {
        state: "none",
        next_delivery: "blocked",
      },
    };
    const draft = defaultTargetDraftFromSession(session, "https://chatgpt.com/c/current-chat");

    expect(draft).toEqual({
      mode: "existing",
      url: "https://chatgpt.com/c/current-chat",
    });
    expect(targetDraftMatchesSaved({
      mode: draft.mode,
      url: draft.url,
      saved: savedTargetDraftFromSession(session),
    })).toBe(false);
  });

  it("does not treat a stale last_tab_url as a saveable current browser chat", () => {
    const session = {
      delivery_target: {
        state: "none",
        next_delivery: "blocked",
      },
      last_tab_url: "https://chatgpt.com/c/stale-chat",
    };
    const liveCurrent = liveBrowserConversationUrlFromSession(session);
    const draft = defaultTargetDraftFromSession(session, liveCurrent);

    expect(liveCurrent).toBe("");
    expect(draft).toEqual({
      mode: "existing",
      url: "",
    });
    expect(targetDraftMatchesSaved({
      mode: draft.mode,
      url: draft.url,
      saved: savedTargetDraftFromSession(session),
    })).toBe(true);
  });

  it("uses a live inspected tab_url as the saveable current browser chat", () => {
    const session = {
      delivery_target: {
        state: "none",
        next_delivery: "blocked",
      },
      tab_url: "https://chatgpt.com/c/live-chat",
      last_tab_url: "https://chatgpt.com/c/stale-chat",
    };
    const liveCurrent = liveBrowserConversationUrlFromSession(session);
    const draft = defaultTargetDraftFromSession(session, liveCurrent);

    expect(liveCurrent).toBe("https://chatgpt.com/c/live-chat");
    expect(draft).toEqual({
      mode: "existing",
      url: "https://chatgpt.com/c/live-chat",
    });
    expect(targetDraftMatchesSaved({
      mode: draft.mode,
      url: draft.url,
      saved: savedTargetDraftFromSession(session),
    })).toBe(false);
  });
});
