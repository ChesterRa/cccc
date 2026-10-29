import { expect, it, vi } from "vite-plus/test";
import { createPendingReplyStore, replyKey } from "./store";

it("keeps unsent drafts separate by Group and request", () => {
  const store = createPendingReplyStore();
  const first = replyKey("a::b", "c");
  const second = replyKey("a", "b::c");
  expect(first).not.toBe(second);
  store.getState().setDraft(first, { selection: { kind: "other" }, text: "未发送" });
  expect(store.getState().entries[first].draft.text).toBe("未发送");
  expect(store.getState().entries[second]).toBeUndefined();
  store.getState().clear(first);
  expect(store.getState().entries[first]).toBeUndefined();
});

it("retries an uncertain answer with the identical client ID and preserves the draft", () => {
  const store = createPendingReplyStore();
  const key = replyKey("group", "request");
  const newId = vi.fn(() => "first-attempt");
  const answer = { v: 1 as const, text: "  response\n" };
  store.getState().setDraft(key, { selection: { kind: "other" }, text: answer.text });
  expect(store.getState().begin(key, answer, newId)?.clientId).toBe("first-attempt");
  expect(store.getState().begin(key, answer, newId)).toBeNull();
  store.getState().fail(key, "first-attempt", "NETWORK_ERROR", true);
  store.getState().setDraft(key, { selection: { kind: "other" }, text: "different" });
  store.getState().clear(key);
  expect(store.getState().entries[key].draft.text).toBe(answer.text);
  expect(store.getState().begin(key, { v: 1, text: "different" }, newId)).toBeNull();
  expect(store.getState().begin(key, answer, newId)?.clientId).toBe("first-attempt");
  expect(newId).toHaveBeenCalledOnce();
  store.getState().complete(key, "some-other-id");
  expect(store.getState().entries[key]).toBeDefined();
  store.getState().complete(key, "first-attempt");
  expect(store.getState().entries[key]).toMatchObject({ closed: "answered", attempt: null });
  expect(store.getState().begin(key, answer, newId)).toBeNull();
});

it("allows editing after a definite rejection", () => {
  const store = createPendingReplyStore();
  const key = replyKey("g", "e");
  store.getState().setDraft(key, { selection: { kind: "option", id: "A" }, text: "" });
  store.getState().begin(key, { v: 1, option_id: "A" }, () => "id");
  store.getState().fail(key, "id", "invalid_answer", false);
  store.getState().setDraft(key, { selection: { kind: "option", id: "B" }, text: "" });
  expect(store.getState().entries[key].draft.selection).toEqual({ kind: "option", id: "B" });
  expect(store.getState().entries[key].attempt).toBeNull();
});
