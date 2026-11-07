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

it("finds non-ASCII path components without taking adjacent prose", () => {
  const examples = [
    {
      root: "/home/dodd/dev/maniau",
      text: "WSL 里的路径：/home/dodd/dev/maniau/.local/video-prep-20261107/to-maker/全片-v8c.mp4",
      paths: [".local/video-prep-20261107/to-maker/全片-v8c.mp4"],
    },
    { root: "/home/项目", text: "看/home/项目/文件/说明.md里的内容", paths: ["文件/说明.md"] },
    { root: "/repo", text: "改了docs/说明.md里的说明", paths: ["docs/说明.md"] },
    { root: "/repo", text: "docs/a.md和docs/b.md", paths: ["docs/a.md", "docs/b.md"] },
    { root: "/repo", text: "更新了CHANGELOG和docs/guide.md", paths: ["docs/guide.md"] },
    { root: "/repo", text: "foo.md/视频.mp4", paths: ["foo.md/视频.mp4"] },
    { root: "C:\\Repo", text: "C:\\Repo\\视频\\全片.mp4", paths: ["视频/全片.mp4"] },
  ];
  for (const { root, text, paths } of examples) {
    const ranges = findMessagePathRanges(text, root);
    expect(
      ranges.map((range) => range.path),
      text,
    ).toEqual(paths);
  }
});

it("does not misidentify Unicode path fragments, extended names, or URLs", () => {
  for (const text of [
    "docs/全片.mp4x",
    "视频/全片.mp4",
    "/other/目录/全片.mp4",
    "../docs/全片.mp4",
    "~/.cccc/说明.md",
    "https://example.test/docs/说明.md",
  ]) {
    expect(findMessagePathRanges(text, "/repo"), text).toEqual([]);
  }
});

it("leaves a Windows network path as text in a POSIX workspace", () => {
  const text = String.raw`电脑上直接打开：\\wsl.localhost\Ubuntu-24.04\home\dodd\dev\maniau.local\video-prep-20261107\to-maker\全片-v8c.mp4`;
  expect(findMessagePathRanges(text, "/home/dodd/dev/maniau")).toEqual([]);
});

it("ends a Unicode path before fullwidth parentheses", () => {
  const text =
    "其他：文件：.local/video-prep-20261107/remotion/out/to-maker/全片-v8c.mp4（2 分 59.5 秒，在 3 分钟以内）";
  const ranges = findMessagePathRanges(text, "/home/dodd/dev/maniau");
  expect(ranges.map((range) => [text.slice(range.start, range.end), range.path])).toEqual([
    [
      ".local/video-prep-20261107/remotion/out/to-maker/全片-v8c.mp4",
      ".local/video-prep-20261107/remotion/out/to-maker/全片-v8c.mp4",
    ],
  ]);
});

it("uses the exact Unicode path bounds and leaves line numbers and prose outside", () => {
  const text = "😀看docs/𠮷野/说明.md:9和/repo/视频/全片.mp4的结果";
  const ranges = findMessagePathRanges(text, "/repo");
  expect(ranges.map((range) => [text.slice(range.start, range.end), range.path])).toEqual([
    ["docs/𠮷野/说明.md", "docs/𠮷野/说明.md"],
    ["/repo/视频/全片.mp4", "视频/全片.mp4"],
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
