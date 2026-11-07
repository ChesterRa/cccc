import { expect, it } from "vite-plus/test";
import { getActorAccentIndex, type Actor } from "../types";
import {
  findMessageLinkRanges,
  findMessageMentionRanges,
  getMessageMentionTokens,
} from "./mentionHighlight";

it("keeps URL punctuation and balanced parentheses in the right places", () => {
  const text = "看 https://example.test/a(b)?q=1，另看 (https://example.test/end).";
  expect(findMessageLinkRanges(text).map((range) => text.slice(range.start, range.end))).toEqual([
    "https://example.test/a(b)?q=1",
    "https://example.test/end",
  ]);
  expect(findMessageLinkRanges("alice@www.example.test https:// invalid://example.test")).toEqual(
    [],
  );
  const long = "https://example.test/path" + ")".repeat(10_000);
  const [link] = findMessageLinkRanges(long);
  expect(long.slice(link.start, link.end)).toBe("https://example.test/path");
});

const actors = [
  { id: "claude-1", title: "Claude" },
  { id: "codex-1", title: "CCCC开发监督" },
] as Actor[];

function marked(text: string, tokens = getMessageMentionTokens(actors, undefined)) {
  return findMessageMentionRanges(text, tokens).map(
    (range) => `${range.kind}:${text.slice(range.start, range.end)}`,
  );
}

it("marks this Group's agents by id or title and the composer's selectors", () => {
  expect(marked("@claude-1 and @Claude, then @all, @foreman, @peers and @chet.")).toEqual([
    "agent:@claude-1",
    "agent:@Claude",
    "agent:@all",
    "agent:@foreman",
    "agent:@peers",
    "agent:@chet",
  ]);
});

it("leaves issue numbers, code and unknown names as text", () => {
  expect(
    marked(
      "#34 的文案已改，见 #31 和 #30。Fixes #1, #include <stdio.h>, @decorator, @someone-else",
    ),
  ).toEqual([]);
});

it("marks a Group only when the message carries its reference", () => {
  const tokens = getMessageMentionTokens(actors, {
    refs: [
      { kind: "local_group_route", token: "#Target Repo" },
      { kind: "task_ref", token: "#Other" },
    ],
  });
  expect(marked("Ask #Target Repo, not #Other.", tokens)).toEqual(["group:#Target Repo"]);
});

it("skips email addresses and links", () => {
  expect(marked("Mail claude-1@example.com or see https://example.com/@claude-1#all")).toEqual([]);
  const tokens = getMessageMentionTokens([{ id: "例子" }, { id: "example" }] as Actor[], undefined);
  expect(marked("名字@例子.中国 名字@example.com", tokens)).toEqual([]);
  expect(marked("请@例子 看一下，再问@example。", tokens)).toEqual([
    "agent:@例子",
    "agent:@example",
  ]);
});

it("needs a boundary on both sides of a name", () => {
  expect(marked("@claude-10 x@claude-1 @claude-1x @claude-1/notes")).toEqual([]);
  expect(marked("请@claude-1看一下（@codex-1）。")).toEqual(["agent:@claude-1", "agent:@codex-1"]);
  expect(marked("请@codex-1看一下README.md")).toEqual(["agent:@codex-1"]);
  expect(marked("@CCCC开发监督 好的，@CCCC开发监督们")).toEqual(["agent:@CCCC开发监督"]);
});

it("prefers the longest known name", () => {
  const tokens = getMessageMentionTokens(
    [{ id: "claude" }, { id: "claude-1" }] as Actor[],
    undefined,
  );
  expect(marked("@claude-1 and @claude", tokens)).toEqual(["agent:@claude-1", "agent:@claude"]);
});

it("gives an agent's references the accent slot of its name, and nothing else one", () => {
  const tokens = getMessageMentionTokens(actors, {
    refs: [{ kind: "local_group_route", token: "#Target Repo" }],
  });
  const text = "@claude-1 @Claude @CCCC开发监督 @all #Target Repo";
  expect(findMessageMentionRanges(text, tokens).map((range) => range.accent)).toEqual([
    getActorAccentIndex("claude-1"),
    getActorAccentIndex("claude-1"),
    getActorAccentIndex("codex-1"),
    undefined,
    undefined,
  ]);
});

it("keeps a selector neutral when an agent has the same name", () => {
  const tokens = getMessageMentionTokens([{ id: "all" }] as Actor[], undefined);
  expect(findMessageMentionRanges("@all", tokens)[0].accent).toBeUndefined();
});

it("colours an exact id as its own agent even when another agent has that title", () => {
  for (const order of [
    [
      { id: "first-agent", title: "codex-1" },
      { id: "codex-1", title: "Other" },
    ],
    [
      { id: "codex-1", title: "Other" },
      { id: "first-agent", title: "codex-1" },
    ],
  ] as Actor[][]) {
    const tokens = getMessageMentionTokens(order, undefined);
    expect(findMessageMentionRanges("@codex-1", tokens)[0].accent).toBe(
      getActorAccentIndex("codex-1"),
    );
  }
});

it("leaves a title that several agents share neutral", () => {
  const tokens = getMessageMentionTokens(
    [
      { id: "a", title: "Reviewer" },
      { id: "b", title: "Reviewer" },
    ] as Actor[],
    undefined,
  );
  const ranges = findMessageMentionRanges("@Reviewer and @a", tokens);
  expect(ranges.map((range) => range.accent)).toEqual([undefined, getActorAccentIndex("a")]);
  const folded = getMessageMentionTokens(
    [
      { id: "a", title: "Review" },
      { id: "b", title: "review" },
    ] as Actor[],
    undefined,
  );
  expect(findMessageMentionRanges("@Review @review", folded).map((range) => range.accent)).toEqual([
    undefined,
    undefined,
  ]);
  const spelled = getMessageMentionTokens(
    [
      { id: "a", title: "@Review" },
      { id: "b", title: "Review" },
    ] as Actor[],
    undefined,
  );
  expect(findMessageMentionRanges("@Review", spelled)[0].accent).toBeUndefined();
});
