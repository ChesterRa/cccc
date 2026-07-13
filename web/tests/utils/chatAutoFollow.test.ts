import { describe, expect, it } from "vite-plus/test";

import {
  getChatTailMutationSnapshot,
  getChatTailSnapshot,
  shouldAutoFollowOnTailAppend,
  shouldAutoFollowOnTailMutation,
} from "../../src/utils/chatAutoFollow";

describe("chatAutoFollow", () => {
  it("auto-follows real tail append", () => {
    const prev = getChatTailSnapshot("m-1", 1);
    const next = getChatTailSnapshot("m-2", 2);

    expect(shouldAutoFollowOnTailAppend(prev, next)).toBe(true);
  });

  it("does not auto-follow history prepend that keeps the same tail", () => {
    const prev = getChatTailSnapshot("m-9", 9);
    const next = getChatTailSnapshot("m-9", 12);

    expect(shouldAutoFollowOnTailAppend(prev, next)).toBe(false);
  });

  it("does not auto-follow first load or non-growing lists", () => {
    expect(
      shouldAutoFollowOnTailAppend(getChatTailSnapshot(null, 0), getChatTailSnapshot("m-1", 1)),
    ).toBe(false);
    expect(
      shouldAutoFollowOnTailAppend(getChatTailSnapshot("m-2", 2), getChatTailSnapshot("m-3", 2)),
    ).toBe(false);
  });

  it("auto-follows same-tail content mutation", () => {
    const prev = getChatTailMutationSnapshot("m-2", "m-2|10|0");
    const next = getChatTailMutationSnapshot("m-2", "m-2|24|0");

    expect(shouldAutoFollowOnTailMutation(prev, next)).toBe(true);
  });

  it("does not treat real tail append as a tail mutation", () => {
    const prev = getChatTailMutationSnapshot("m-2", "m-2|10|0");
    const next = getChatTailMutationSnapshot("m-3", "m-3|4|0");

    expect(shouldAutoFollowOnTailMutation(prev, next)).toBe(false);
  });

  it("does not treat history prepend metadata as a tail mutation when the tail content is unchanged", () => {
    const prev = getChatTailMutationSnapshot("m-9", "m-9|assistant|t9||24|1");
    const next = getChatTailMutationSnapshot("m-9", "m-9|assistant|t9||24|1");

    expect(shouldAutoFollowOnTailMutation(prev, next)).toBe(false);
  });
});
