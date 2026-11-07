import { expect, it, vi } from "vite-plus/test";
import { messageLinkPreview } from "./messageLinkPreview";

vi.mock("../services/api/base", () => ({ withAuthToken: (url: string) => `${url}#signed` }));

const ORIGIN = "https://cccc.example";
const kindOf = (href: string) => messageLinkPreview(href, ORIGIN)?.kind ?? null;

it("previews media on any site, where the browser can show it without reading it", () => {
  expect(kindOf("https://example.test/a/photo.PNG?size=2")).toBe("image");
  expect(kindOf("https://example.test/clip.mp4#t=3")).toBe("video");
  expect(kindOf("https://example.test/voice.mp3")).toBe("audio");
});

it("leaves another site's documents as links unless that site lets the page read them", () => {
  expect(kindOf("https://example.test/report.pdf")).toBeNull();
  expect(kindOf("https://example.test/notes.md")).toBeNull();
  expect(kindOf("https://github.com/o/r/blob/main/README.md")).toBeNull();
  expect(kindOf("https://raw.githubusercontent.com/o/r/main/README.md")).toBe("markdown");
  expect(kindOf("https://raw.githubusercontent.com/o/r/main/src/main.rs")).toBe("text");
  expect(kindOf("https://raw.githubusercontent.com/o/r/main/paper.pdf")).toBeNull();
});

it("leaves web pages and other addresses alone", () => {
  expect(kindOf("https://example.test/")).toBeNull();
  expect(kindOf("https://example.test/page.html")).toBeNull();
  expect(kindOf("https://example.test/issues/42")).toBeNull();
  expect(kindOf("mailto:someone@example.test")).toBeNull();
  expect(kindOf("javascript:alert(1)//x.png")).toBeNull();
  expect(kindOf("data:image/png;base64,AAAA")).toBeNull();
  expect(kindOf("http://[bad")).toBeNull();
});

it("reads this site's attachments by the name they carry, and signs the address", () => {
  const preview = messageLinkPreview(
    `${ORIGIN}/api/v1/groups/g1/blobs/0123abcd?filename=report.pdf`,
    ORIGIN,
  );

  expect(preview?.kind).toBe("pdf");
  expect(preview?.label).toBe("report.pdf");
  expect(preview?.href.endsWith("#signed")).toBe(true);
  expect(preview?.downloadHref).toContain("download=true");
});

it("previews a copied download link instead of downloading it", () => {
  const preview = messageLinkPreview(
    `${ORIGIN}/api/v1/groups/g1/blobs/0123abcd?filename=report.pdf&download=true`,
    ORIGIN,
  );

  expect(preview?.href).not.toContain("download=true");
  expect(preview?.downloadHref).toContain("download=true");
});

it("opens only the Group's files from this site", () => {
  expect(kindOf(`${ORIGIN}/api/v1/groups/g1/workspace/content?path=docs/notes.md`)).toBe(
    "markdown",
  );
  expect(kindOf(`${ORIGIN}/api/v1/groups/g1/settings?filename=notes.md`)).toBeNull();
  expect(kindOf(`${ORIGIN}/ui/assets/logo.png`)).toBeNull();
});

it("does not take a file name from another site's parameters", () => {
  expect(kindOf("https://example.test/download?filename=photo.png")).toBeNull();
});

it("shows the file's readable name", () => {
  expect(messageLinkPreview("https://example.test/a%20b.png", ORIGIN)?.label).toBe("a b.png");
});
