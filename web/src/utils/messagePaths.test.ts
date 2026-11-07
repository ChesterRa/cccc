import { expect, it } from "vite-plus/test";
import {
  findMessageInlinePathRanges,
  findMessagePathRanges,
  messageHrefPath,
} from "./messagePaths";

it("finds relative and in-scope absolute files, leaving line numbers outside", () => {
  const text = "路径:crates/web/main.rs:42:7，看/repo/docs/readme.md的第三节。";
  const ranges = findMessagePathRanges(text, "/repo");
  expect(ranges.map((range) => [text.slice(range.start, range.end), range.path])).toEqual([
    ["crates/web/main.rs", "crates/web/main.rs"],
    ["/repo/docs/readme.md", "docs/readme.md"],
  ]);
});

it("leaves URLs, outside paths, traversal and unsupported names as text", () => {
  const text =
    "https://example.test/docs/a.md /other/a.md ../secrets/a.md ~/.cccc/a.md name.md docs/data.exe";
  expect(findMessagePathRanges(text, "/repo")).toEqual([]);
  expect(findMessagePathRanges("docs/a.md", "")).toEqual([]);
});

it("accepts Windows separators only for a Windows workspace", () => {
  const text = "C:\\Repo\\src\\main.rs:12 src\\other.ts";
  expect(findMessagePathRanges(text, "C:\\repo").map((range) => range.path)).toEqual([
    "src/main.rs",
    "src/other.ts",
  ]);
  expect(findMessagePathRanges(text, "/repo")).toEqual([]);
});

it("does not turn an absolute path split by spaces into a relative file", () => {
  expect(findMessagePathRanges("C:\\Other Root\\docs\\a.md", "C:\\Repo")).toEqual([]);
  expect(
    findMessagePathRanges("看 /other root/docs/a.md，然后 docs/ok.md", "/repo").map(
      (item) => item.path,
    ),
  ).toEqual(["docs/ok.md"]);
  expect(findMessagePathRanges("~/home folder/docs/a.md", "/repo")).toEqual([]);
  expect(
    findMessagePathRanges("~/.cccc/a.md docs/readme.md", "/repo").map((item) => item.path),
  ).toEqual(["docs/readme.md"]);
});

it("keeps spaces in an inline-code path without swallowing separate paths or prose", () => {
  const text = "C:\\Program Files\\repo\\notes\\my report.md:7";
  expect(findMessageInlinePathRanges(text, "C:\\Program Files\\repo")).toEqual([
    { start: 0, end: text.length - 2, path: "notes/my report.md" },
  ]);
  expect(
    findMessageInlinePathRanges("docs/a.md docs/b.md", "/repo").map((item) => item.path),
  ).toEqual(["docs/a.md", "docs/b.md"]);
  expect(findMessageInlinePathRanges("read docs/a.md", "/repo").map((item) => item.path)).toEqual([
    "docs/a.md",
  ]);
  expect(findMessageInlinePathRanges("docs/说明 一.md:9", "/repo")).toEqual([
    { start: 0, end: "docs/说明 一.md".length, path: "docs/说明 一.md" },
  ]);
  expect(
    findMessageInlinePathRanges("C:\\Other Root\\docs\\a.md", "C:\\Program Files\\repo"),
  ).toEqual([]);
  expect(findMessageInlinePathRanges("/other root/docs/a.md", "/repo")).toEqual([]);
});

it("reads workspace files from Markdown link targets", () => {
  expect(messageHrefPath("docs/guide.md", "/repo")).toBe("docs/guide.md");
  expect(messageHrefPath("README.md#intro", "/repo")).toBe("README.md");
  expect(messageHrefPath("docs/guide.md:12:3", "/repo")).toBe("docs/guide.md");
  expect(messageHrefPath("/repo/docs/a%20b.md?x=1", "/repo")).toBe("docs/a b.md");
  expect(messageHrefPath("C:/Repo/docs/a.md", "C:\\repo")).toBe("docs/a.md");
});

it("leaves every other link target alone", () => {
  for (const href of [
    "https://example.test/a.md",
    "//example.test/a.md",
    "#intro",
    "mailto:a@example.test",
    "javascript:alert(1)//a.md",
    "/other/a.md",
    "../a.md",
    "~/.cccc/a.md",
    "docs/data.exe",
    "docs/%E0%A4%A.md",
  ])
    expect(messageHrefPath(href, "/repo")).toBeNull();
  expect(messageHrefPath("docs/a.md", "")).toBeNull();
});

it("includes pages, which Files shows as static HTML", () => {
  expect(findMessagePathRanges("see docs/design/index.html", "/repo").map((r) => r.path)).toEqual([
    "docs/design/index.html",
  ]);
});
