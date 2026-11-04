import { describe, expect, it } from "vite-plus/test";
import {
  createComposerGroupMentionToken,
  pruneComposerGroupMentionTokens,
} from "../../hooks/composerGroupMentions";
import {
  findComposerMentionAtCaret,
  rebaseComposerMentionTokens,
  replaceComposerMention,
} from "./composerMentionInput";

const trigger = (text: string) => findComposerMentionAtCaret(text, text.length);

describe("mention trigger boundaries", () => {
  it.each([
    "user@example.com",
    "名字@例子.中国",
    "名字@例子.中国。",
    "https://example.com/#section",
    "https://example.com/?#section",
    "task#123",
    "one\\@two",
  ])("does not treat %s as a mention trigger", (text) => expect(trigger(text)).toBeNull());
  it.each([
    "@",
    "look @work",
    "(@work",
    "请@wo",
    "请#gr",
    "日本語@wo",
    "かな@wo",
    "한@wo",
    "请看，@work",
    "请看（@work",
    "please #group",
  ])("detects a mention after a boundary: %s", (text) => expect(trigger(text)).not.toBeNull());
  it("replaces at the caret, keeps the suffix, and relocates an existing suffix token", () => {
    const oldText = "hello @wo and #group";
    const at = oldText.indexOf("@wo");
    expect(findComposerMentionAtCaret(oldText, at + 3)).toEqual({
      kind: "agent",
      start: at,
      end: at + 3,
      filter: "wo",
    });
    const oldGroup = { start: oldText.indexOf("#group"), end: oldText.length, token: "#group" };
    const next = replaceComposerMention(oldText, { start: at, end: at + 3 }, "@worker");
    expect(next.text).toBe("hello @worker and #group");
    expect(rebaseComposerMentionTokens([oldGroup], next.edit)).toEqual([
      { start: next.text.indexOf("#group"), end: next.text.length, token: "#group" },
    ]);
  });

  it.each([
    ["(#gr)", 1, 4, "(#group)", 1],
    ["请看（#gr）", 3, 6, "请看（#group）", 3],
  ])(
    "preserves existing punctuation and keeps the selected Group reference in %s",
    (before, start, end, expected, tokenStart) => {
      const inserted = replaceComposerMention(before, { start, end }, "#group");
      expect(inserted.text).toBe(expected);
      const token = createComposerGroupMentionToken({
        groupId: "g_target",
        token: "#group",
        start: tokenStart,
      });
      expect(pruneComposerGroupMentionTokens({ text: inserted.text, tokens: [token!] })).toEqual([
        token,
      ]);
    },
  );

  it("does not keep a reference joined to an identifier", () => {
    const token = createComposerGroupMentionToken({
      groupId: "g_target",
      token: "#group",
      start: 0,
    });
    expect(pruneComposerGroupMentionTokens({ text: "#group-2", tokens: [token!] })).toEqual([]);
  });

  it("keeps a reference after CJK text, but drops it if CJK text is joined after the token", () => {
    const inserted = replaceComposerMention("请#gr 后续", { start: 1, end: 4 }, "#group");
    expect(inserted.text).toBe("请#group 后续");
    const token = createComposerGroupMentionToken({
      groupId: "g_target",
      token: "#group",
      start: 1,
    });
    expect(pruneComposerGroupMentionTokens({ text: inserted.text, tokens: [token!] })).toEqual([
      token,
    ]);
    expect(pruneComposerGroupMentionTokens({ text: "请#group后续", tokens: [token!] })).toEqual([]);
  });
});
