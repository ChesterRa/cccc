import { expect, it } from "vite-plus/test";

import { messagePathTarget } from "./messagePathContext";

const target = { scopeKey: "s1", rootPath: "/repo", open: () => {} };

it("opens paths from this workspace, or from a message that names no scope", () => {
  expect(messagePathTarget(target, { scopeKey: "s1", foreign: false })).toBe(target);
  expect(messagePathTarget(target, { scopeKey: "", foreign: false })).toBe(target);
});

it("leaves paths alone where they would mean a different file", () => {
  expect(messagePathTarget(target, { scopeKey: "s2", foreign: false })).toBeNull();
  expect(messagePathTarget(target, { scopeKey: "", foreign: true })).toBeNull();
});

it("needs a workspace", () => {
  expect(messagePathTarget(null, { scopeKey: "", foreign: false })).toBeNull();
});
