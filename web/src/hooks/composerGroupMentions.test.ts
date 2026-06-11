import { describe, expect, it } from "vitest";

import type { GroupMeta } from "../types";
import {
  createComposerAgentMentionToken,
  createComposerGroupMentionToken,
  extractControlledGroupMentionTargetActor,
  pruneComposerAgentMentionTokens,
  pruneComposerGroupMentionTokens,
  resolveControlledComposerMentionContext,
  resolveSelectedComposerGroupMention,
} from "./composerGroupMentions";

const groups = [
  { group_id: "g_local", title: "Local" },
  { group_id: "self-agent", title: "Self Agent" },
] as unknown as GroupMeta[];

describe("composer group mention tokens", () => {
  it("keeps only menu-selected group tokens that still match the text range", () => {
    const text = "ask #Self Agent to help";
    const token = createComposerGroupMentionToken({ groupId: "self-agent", token: "#Self Agent", start: 4 });
    expect(token).not.toBeNull();
    expect(pruneComposerGroupMentionTokens({ text, tokens: [token!] })).toEqual([token]);
    expect(pruneComposerGroupMentionTokens({ text: "ask #Self Agents to help", tokens: [token!] })).toEqual([]);
  });

  it("resolves the last selected live group token", () => {
    const text = "#Self Agent first #Self Agent second";
    const first = createComposerGroupMentionToken({ groupId: "self-agent", token: "#Self Agent", start: 0 })!;
    const second = createComposerGroupMentionToken({ groupId: "self-agent", token: "#Self Agent", start: 18 })!;
    expect(resolveSelectedComposerGroupMention({ text, selectedGroupId: "g_local", groups, tokens: [first, second] })).toEqual(second);
  });

  it("ignores copied or typed # text when it was never selected from the menu", () => {
    expect(resolveSelectedComposerGroupMention({
      text: "copied #Self Agent text",
      selectedGroupId: "g_local",
      groups,
      tokens: [],
    })).toBeNull();
  });

  it("keeps only menu-selected agent tokens that still match the text range", () => {
    const text = "ask @target to help";
    const token = createComposerAgentMentionToken({ actorId: "target", token: "@target", start: 4, scope: "selected" });
    expect(token).not.toBeNull();
    expect(pruneComposerAgentMentionTokens({ text, tokens: [token!] })).toEqual([token]);
    expect(pruneComposerAgentMentionTokens({ text: "ask @targetx to help", tokens: [token!] })).toEqual([]);
  });

  it("uses only selected # tokens to switch @ suggestions to destination scope", () => {
    const text = "copied #Self Agent @local\nask #Self Agent @remote";
    const selected = createComposerGroupMentionToken({ groupId: "self-agent", token: "#Self Agent", start: text.lastIndexOf("#Self Agent") })!;
    expect(resolveControlledComposerMentionContext({
      text,
      atIndex: text.indexOf("@local"),
      tokens: [selected],
    })).toEqual({ scope: "selected", mentionTargetGroupId: "" });
    expect(resolveControlledComposerMentionContext({
      text,
      atIndex: text.indexOf("@remote"),
      tokens: [selected],
    })).toEqual({ scope: "destination", mentionTargetGroupId: "self-agent" });
  });

  it("extracts target actor only from selected live agent tokens after a selected live group token", () => {
    const text = "copied #Self Agent @wrong\nask #Self Agent @right";
    const selected = createComposerGroupMentionToken({ groupId: "self-agent", token: "#Self Agent", start: text.lastIndexOf("#Self Agent") })!;
    const target = createComposerAgentMentionToken({
      actorId: "right",
      token: "@right",
      start: text.indexOf("@right"),
      scope: "destination",
    })!;
    expect(extractControlledGroupMentionTargetActor({ text, token: selected, agentTokens: [target] })).toBe("right");
    expect(extractControlledGroupMentionTargetActor({ text, token: selected, agentTokens: [] })).toBe("");
    expect(extractControlledGroupMentionTargetActor({ text, token: null })).toBe("");
  });
});
