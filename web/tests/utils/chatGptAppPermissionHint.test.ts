import { beforeEach, describe, expect, it, vi } from "vitest";

import {
  CHATGPT_APP_PERMISSION_HINT_DISMISSED_EVENT,
  CHATGPT_APP_PERMISSION_HINT_DISMISSED_KEY,
  dismissChatGptAppPermissionHint,
  readChatGptAppPermissionHintDismissed,
} from "../../src/utils/chatGptAppPermissionHint";

function makeStorage() {
  const store = new Map<string, string>();
  return {
    getItem: (key: string) => store.get(key) ?? null,
    setItem: (key: string, value: string) => {
      store.set(key, value);
    },
    clear: () => {
      store.clear();
    },
  };
}

describe("chatGptAppPermissionHint", () => {
  const localStorageMock = makeStorage();
  const dispatchEvent = vi.fn();

  beforeEach(() => {
    localStorageMock.clear();
    dispatchEvent.mockClear();
    vi.stubGlobal("window", {
      localStorage: localStorageMock,
      dispatchEvent,
    });
  });

  it("persists and broadcasts dismissal", () => {
    expect(readChatGptAppPermissionHintDismissed()).toBe(false);

    dismissChatGptAppPermissionHint();

    expect(readChatGptAppPermissionHintDismissed()).toBe(true);
    expect(localStorageMock.getItem(CHATGPT_APP_PERMISSION_HINT_DISMISSED_KEY)).toBe("1");
    expect(dispatchEvent).toHaveBeenCalledTimes(1);
    expect(dispatchEvent.mock.calls[0]?.[0]).toBeInstanceOf(Event);
    expect((dispatchEvent.mock.calls[0]?.[0] as Event).type).toBe(CHATGPT_APP_PERMISSION_HINT_DISMISSED_EVENT);
  });
});
