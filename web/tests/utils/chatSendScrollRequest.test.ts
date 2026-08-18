import { describe, expect, it } from "vite-plus/test";

import {
  consumeChatSendScrollRequest,
  createChatSendScrollRequest,
  invalidateChatSendScrollRequestForOwner,
  isChatSendScrollRequestOwner,
  shouldExecuteChatSendScroll,
} from "../../src/utils/chatSendScrollRequest";

describe("group/view-owned chat send scroll requests", () => {
  it("owns a request only for its exact group and view", () => {
    const request = createChatSendScrollRequest(1, "group-a", "view-a");

    expect(isChatSendScrollRequestOwner(request, "group-a", "view-a")).toBe(true);
    expect(isChatSendScrollRequestOwner(request, "group-b", "view-a")).toBe(false);
    expect(isChatSendScrollRequestOwner(request, "group-a", "view-b")).toBe(false);
  });

  it("consumes an exact request so switching back cannot replay it", () => {
    const request = createChatSendScrollRequest(1, "group-a", "view-a");

    expect(consumeChatSendScrollRequest(request, 1)).toBeNull();
    expect(consumeChatSendScrollRequest(request, 2)).toBe(request);
    expect(invalidateChatSendScrollRequestForOwner(request, "group-b", "view-b")).toBeNull();
    expect(invalidateChatSendScrollRequestForOwner(request, "group-a", "view-b")).toBeNull();
  });

  it("revalidates owner and generation between the outer and inner animation frames", () => {
    const request = createChatSendScrollRequest(1, "group-a", "view-a");
    const base = {
      request,
      activeRequest: request,
      requestToken: 7,
      currentRequestToken: 7,
      groupId: "group-a",
      viewKey: "view-a",
    };

    expect(shouldExecuteChatSendScroll(base)).toBe(true);
    expect(shouldExecuteChatSendScroll({ ...base, currentRequestToken: 8 })).toBe(false);
    expect(shouldExecuteChatSendScroll({ ...base, activeRequest: null })).toBe(false);
    expect(shouldExecuteChatSendScroll({ ...base, groupId: "group-b" })).toBe(false);
    expect(shouldExecuteChatSendScroll({ ...base, viewKey: "view-b" })).toBe(false);
  });

  it("allows a new group request exactly once with its own generation", () => {
    const requestA = createChatSendScrollRequest(1, "group-a", "view-a");
    const requestB = createChatSendScrollRequest(2, "group-b", "view-b");

    expect(
      shouldExecuteChatSendScroll({
        request: requestB,
        activeRequest: requestB,
        requestToken: 3,
        currentRequestToken: 3,
        groupId: "group-b",
        viewKey: "view-b",
      }),
    ).toBe(true);
    expect(
      shouldExecuteChatSendScroll({
        request: requestB,
        activeRequest: null,
        requestToken: 3,
        currentRequestToken: 3,
        groupId: "group-b",
        viewKey: "view-b",
      }),
    ).toBe(false);
    expect(isChatSendScrollRequestOwner(requestA, "group-b", "view-b")).toBe(false);
  });
});
