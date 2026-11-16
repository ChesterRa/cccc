import { expect, it } from "vite-plus/test";
import {
  builtinReplyDefault,
  effectiveReplyTemplates,
  replyEntriesOf,
  templatePreview,
} from "./effective";

const en = (key: string) => `en:${key}`;
const ja = (key: string) => `ja:${key}`;

it("keeps the saved order, with default replies as ordinary entries in the interface language", () => {
  const replies = effectiveReplyTemplates(en, [
    { id: "b", name: "B", text: "\n  exact body\n" },
    { builtin: "why" },
    { id: "a", name: "A", text: "a" },
    { builtin: "proceed" },
  ]);
  expect(replies.map((reply) => reply.source)).toEqual(["custom:b", "why", "custom:a", "proceed"]);
  expect(replies[1]).toEqual({
    source: "why",
    name: "en:quickReply.why",
    text: "en:quickReply.whyText",
    emoji: "🤔",
  });
  expect(replies[0]).toEqual({ source: "custom:b", name: "B", text: "\n  exact body\n" });
  expect(builtinReplyDefault(ja, "why")).toEqual({
    name: "ja:quickReply.why",
    text: "ja:quickReply.whyText",
  });
});

it("overrides fields independently without translating or trimming saved text", () => {
  const entries = [
    { builtin: "proceed" as const, name: "Check first" },
    { builtin: "why" as const, text: "\n  Why?\n    - evidence\n" },
    { builtin: "rethink" as const },
  ];
  const english = effectiveReplyTemplates(en, entries);
  const japanese = effectiveReplyTemplates(ja, entries);
  expect(english[0].name).toBe("Check first");
  expect(japanese[0].name).toBe("Check first");
  expect(japanese[0].text).toBe("ja:quickReply.proceedText");
  expect(japanese[1].name).toBe("ja:quickReply.why");
  expect(japanese[1].text).toBe("\n  Why?\n    - evidence\n");
  expect(japanese[0].emoji).toBeUndefined();
  expect(japanese[1].emoji).toBeUndefined();
  expect(japanese[2].emoji).toBe("🔄");
});

it("shows only what the list holds: a deleted default reply is simply absent", () => {
  expect(effectiveReplyTemplates(en, [])).toEqual([]);
  expect(
    effectiveReplyTemplates(en, [{ id: "mine", name: "Mine", text: "my body" }]).map(
      (reply) => reply.name,
    ),
  ).toEqual(["Mine"]);
});

it("leaves out a default reply of a kind this page does not know", () => {
  expect(
    replyEntriesOf([
      { builtin: "proceed" },
      { builtin: "later" },
      { id: "x", name: "X", text: "x" },
    ]),
  ).toEqual([{ builtin: "proceed" }, { id: "x", name: "X", text: "x" }]);
});

it("previews the first line with text", () => {
  expect(templatePreview("\n   \n  First line \nsecond")).toBe("First line");
  expect(templatePreview("   ")).toBe("");
});
