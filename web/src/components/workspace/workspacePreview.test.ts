// @vitest-environment happy-dom
import { expect, it } from "vite-plus/test";
import { parseWorkspaceTable, workspaceLink } from "./workspacePreview";
import { staticWorkspaceHtml } from "./workspaceHtml";

it("resolves document links within the workspace without losing filename characters", () => {
  expect(workspaceLink("docs/readme.md", "../images/a%20%23%5Cb.svg#view")).toEqual({
    path: "images/a #\\b.svg",
    hash: "#view",
  });
  expect(workspaceLink("docs/readme.md", "/report.pdf")).toEqual({ path: "report.pdf", hash: "" });
  expect(workspaceLink("docs/readme.md", "#intro")).toEqual({
    path: "docs/readme.md",
    hash: "#intro",
  });
  for (const path of [
    "../../outside.txt",
    "%2e%2e/%2e%2e/outside",
    "https://example.com/file",
    "//example.com/file",
    "javascript:alert(1)",
    "%zz",
    "bad%00.txt",
  ])
    expect(workspaceLink("docs/readme.md", path)).toBeNull();
});

it("parses quoted delimited data including BOM, escaped quotes, multiline cells and empty trailing columns", () => {
  expect(
    parseWorkspaceTable('\uFEFFname,details,\r\nAlice,"one, two\r\nthree ""quoted""",\r\n', ","),
  ).toMatchObject({
    rows: [
      ["name", "details", ""],
      ["Alice", 'one, two\r\nthree "quoted"', ""],
    ],
    error: false,
    limited: false,
  });
  expect(parseWorkspaceTable('x\t"y\tz"\n', "\t").rows).toEqual([["x", "y\tz"]]);
  expect(parseWorkspaceTable("", ",").rows).toEqual([]);
  for (const input of ['"unterminated', 'one"two', '"one"two'])
    expect(parseWorkspaceTable(input, ",").error).toBe(true);
});

it("bounds the rendered table without changing the source or ignoring invalid quoting beyond the visible rows", () => {
  const text = Array.from({ length: 600 }, () => Array(120).fill("value").join(",")).join("\n");
  const table = parseWorkspaceTable(text, ",");
  expect(table.rows).toHaveLength(200);
  expect(table.rows[0]).toHaveLength(50);
  expect(table).toMatchObject({ rowCount: 600, columns: 120, limited: true, error: false });
  expect(parseWorkspaceTable(text + '\n"bad', ",").error).toBe(true);
});

it("prepares static HTML with scoped resources and no active document navigation or script entry points", () => {
  const html = staticWorkspaceHtml(
    '<html><head></head><body style="margin:24px"><base href="https://example.com/"><meta http-equiv="refresh" content="0;url=/"><link rel="stylesheet" href="style.css"><style>h1{color:red}</style><script>parent.compromised=true</script><iframe src="/"></iframe><form action="/"><button formaction="/">Send</button></form><h1 onclick="alert(1)">Report</h1><img src="plot.png" srcset="other.png 2x" onerror="alert(1)"><a href="next.md" ping="/" target="_top">Next</a>',
    (url) => "/scoped?path=" + encodeURIComponent(url),
    "https://cccc.test/scoped",
  );
  expect(html).toContain('style="margin:24px"');
  const template = document.createElement("template");
  template.innerHTML = html;
  const doc = template.content;
  expect(
    doc.querySelector(
      "script, iframe, base, link, [onclick], [onerror], [srcset], [ping], [target], [formaction], [action]",
    ),
  ).toBeNull();
  expect(doc.querySelector("img")?.getAttribute("src")).toBe("/scoped?path=plot.png");
  expect(doc.querySelector("a")?.getAttribute("href")).toBe("/scoped?path=next.md");
  expect(doc.querySelector("meta")?.getAttribute("content")).toContain("script-src 'none'");
  expect(doc.querySelector("style")?.textContent).toContain("color:red");
});

it("keeps empty HTML drafts previewable", () => {
  expect(staticWorkspaceHtml("", (url) => url, "https://cccc.test/scoped")).toContain(
    "Content-Security-Policy",
  );
});

it("adds native controls independently of media source syntax and never restores autoplay", () => {
  const template = document.createElement("template");
  template.innerHTML = staticWorkspaceHtml(
    '<audio autoplay><source src="tone.wav" type="audio/wav"></audio><video autoplay><source src="clip.mp4" type="video/mp4"></video><audio src="direct.wav"></audio><video poster="poster.png"><source src="another.mp4"></video>',
    (url) => "/scoped?file=" + encodeURIComponent(url),
    "https://cccc.test/scoped",
  );
  const media = template.content.querySelectorAll("audio, video");
  expect(media).toHaveLength(4);
  for (const element of media) {
    expect(element.hasAttribute("controls")).toBe(true);
    expect(element.getAttribute("preload")).toBe("metadata");
    expect(element.hasAttribute("autoplay")).toBe(false);
  }
  expect(template.content.querySelector("source")?.getAttribute("src")).toBe(
    "/scoped?file=tone.wav",
  );
});
