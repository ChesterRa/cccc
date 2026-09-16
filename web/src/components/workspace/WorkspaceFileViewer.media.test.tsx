// @vitest-environment happy-dom
import type { Window as HappyWindow } from "happy-dom";
import { act, useState } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, expect, it, vi } from "vite-plus/test";
import type { WorkspaceFile } from "../../types";
import { WorkspaceFileViewer } from "./WorkspaceFileViewer";

vi.mock("react-i18next", () => ({
  useTranslation: () => ({
    t: (key: string, options?: { defaultValue?: string }) => options?.defaultValue ?? key,
  }),
}));

let root: Root;
let host: HTMLDivElement;
const noop = () => undefined;
const file = (path: string, mime: string): WorkspaceFile => ({
  scope_key: "scope-a",
  scope_url: "/project a",
  path,
  mime_type: mime,
  bytes: 3_000_000,
  content: "",
  binary: true,
  truncated: true,
  sha256: "",
});

function Harness({ file, readOnly = false }: { file: WorkspaceFile; readOnly?: boolean }) {
  const [draft, setDraft] = useState(file.content);
  const [reloadVersion, setReloadVersion] = useState(0);
  return (
    <WorkspaceFileViewer
      groupId="g_a"
      file={file}
      draft={draft}
      setDraft={setDraft}
      isDark={false}
      readOnly={readOnly}
      saving={false}
      error=""
      conflict={false}
      onClose={noop}
      onSave={async () => true}
      reloadVersion={reloadVersion}
      onReload={() => setReloadVersion((value) => value + 1)}
      onAttach={noop}
    />
  );
}
const render = (file: WorkspaceFile, readOnly = false) =>
  act(async () => root.render(<Harness file={file} readOnly={readOnly} />));
beforeEach(() => {
  Object.assign(globalThis, { IS_REACT_ACT_ENVIRONMENT: true });
  // Native PDF loading is exercised by the isolated real-browser HTTP probe.
  (window as unknown as HappyWindow).happyDOM.settings.disableIframePageLoading = true;
  host = document.createElement("div");
  document.body.append(host);
  root = createRoot(host);
});
afterEach(async () => {
  await act(async () => root.unmount());
  host.remove();
  (window as unknown as HappyWindow).happyDOM.settings.disableIframePageLoading = false;
});

it.each([
  ["module.ts", "video/vnd.dlna.mpeg-tts"],
  ["types.d.ts", "video/vnd.dlna.mpeg-tts"],
  ["module.mts", "video/vnd.dlna.mpeg-tts"],
  ["playlist.m3u", "audio/x-mpegurl"],
  ["view.tsx", "application/octet-stream"],
  ["module.cts", "application/octet-stream"],
  ["view.jsx", "application/octet-stream"],
  ["module.js", "application/javascript"],
  ["main.py", "text/plain"],
  ["lib.rs", "text/x-rust"],
  ["main.cpp", "text/plain"],
  ["settings.json", "application/json"],
  ["config.yaml", "text/x-yaml"],
  ["run.sh", "application/x-sh"],
  ["Dockerfile", "application/octet-stream"],
  [".env", "application/octet-stream"],
])("opens text in %s without treating an extension hint as media", async (path, mime) => {
  const source = {
    ...file(path, mime),
    binary: false,
    truncated: false,
    content: "export const value = 1;\n",
    bytes: 24,
    sha256: "source-digest",
  };
  await render(source);
  expect(host.querySelector("video, audio, img, iframe")).toBeNull();
  expect(host.querySelector("textarea")?.value).toBe(source.content);
  await render(source, true);
  expect(host.querySelector("textarea")).toBeNull();
  expect(host.querySelector("pre")?.textContent).toBe(source.content);
});

it("keeps oversized TypeScript on the text limit path and binary TS on the video path", async () => {
  await render({ ...file("large.ts", "video/vnd.dlna.mpeg-tts"), binary: false });
  expect(host.querySelector("video, textarea")).toBeNull();
  expect(host.textContent).toContain("too large for the text viewer");
  expect(host.querySelector("a[download]")).not.toBeNull();
  await render({ ...file("stream.ts", "video/vnd.dlna.mpeg-tts"), binary: true });
  expect(host.querySelector("video")?.controls).toBe(true);
});

it("previews large images with the shared viewer and binds download to the opened scope", async () => {
  await render(file("drawing #1.png", "image/png"));
  expect(host.querySelector("[data-graphic-viewer]")).not.toBeNull();
  expect(host.textContent).not.toContain("too large");
  const image = host.querySelector("img")!;
  const query = new URL(image.src).searchParams;
  expect(query.get("path")).toBe("drawing #1.png");
  expect(query.get("scope_key")).toBe("scope-a");
  expect(query.get("scope_url")).toBe("/project a");
  const link = host.querySelector("a")!;
  expect(new URL(link.href).searchParams.get("download")).toBe("true");
  expect(host.querySelector("textarea")).toBeNull();
});

it("uses native video controls and metadata preload, resets failures when files change", async () => {
  await render(file("clip.mp4", "video/mp4"), true);
  const video = host.querySelector("video")!;
  expect(video.controls).toBe(true);
  expect(video.autoplay).toBe(false);
  expect(video.preload).toBe("metadata");
  expect(video.hasAttribute("playsinline")).toBe(true);
  await act(async () => video.dispatchEvent(new Event("error")));
  expect(host.querySelector('[role="alert"]')?.textContent).toBe("workspaceVideoUnavailable");
  expect(host.querySelector("a[download]")).not.toBeNull();
  expect(host.querySelector("video")).toBeNull();
  await render(file("next.webm", "video/webm"));
  expect(host.querySelector('[role="alert"]')).toBeNull();
  expect(host.querySelector("video")?.src).toContain("next.webm");
});

it("keeps SVG source editing and draft preview inert without discarding unsaved edits", async () => {
  const svg = {
    ...file("drawing.svg", "image/svg+xml"),
    binary: false,
    truncated: false,
    content: '<svg xmlns="http://www.w3.org/2000/svg"/>',
    sha256: "old",
  };
  await render(svg);
  await act(async () =>
    host.querySelector<HTMLButtonElement>('[aria-label="workspaceViewSource"]')!.click(),
  );
  const editor = host.querySelector("textarea")!;
  const draft =
    '<svg xmlns="http://www.w3.org/2000/svg" onload="alert(1)"><text>draft</text></svg>';
  await act(async () => {
    Object.getOwnPropertyDescriptor(window.HTMLTextAreaElement.prototype, "value")!.set!.call(
      editor,
      draft,
    );
    editor.dispatchEvent(new Event("input", { bubbles: true }));
  });
  await act(async () =>
    host.querySelector<HTMLButtonElement>('[aria-label="workspacePreview"]')!.click(),
  );
  expect(host.querySelector("img")?.src).toBe(`data:image/svg+xml,${encodeURIComponent(draft)}`);
  expect(host.querySelector("[onload]")).toBeNull();
  await act(async () =>
    host.querySelector<HTMLButtonElement>('[aria-label="workspaceViewSource"]')!.click(),
  );
  expect(host.querySelector("textarea")?.value).toBe(draft);
  await render(file("next.png", "image/png"));
  expect(host.querySelector("textarea")).toBeNull();
  expect(host.querySelector("img")).not.toBeNull();
});

it("does not expose source editing for oversized SVG or pretend other binary files are videos", async () => {
  await render(file("large.svg", "image/svg+xml"));
  expect(host.querySelector('[aria-label="workspaceViewSource"]')).toBeNull();
  expect(host.querySelector("img")).not.toBeNull();
  await render({ ...file("archive.zip", "application/zip"), truncated: false, binary: true });
  expect(host.querySelector("video, img, textarea")).toBeNull();
  expect(host.querySelector("a[download]")).not.toBeNull();
});

it("provides native PDF viewing plus separate-open/download and a fallback when the browser disables inline PDFs", async () => {
  await render({ ...file("report.pdf", "application/pdf"), binary: true });
  expect(host.querySelector("iframe")?.src).toContain("report.pdf");
  expect(host.querySelector('a[target="_blank"]')?.getAttribute("rel")).toContain("noopener");
  expect(host.querySelector("a[download]")?.getAttribute("download")).toBe("report.pdf");
  vi.stubGlobal("navigator", { pdfViewerEnabled: false });
  try {
    await render(file("another.pdf", "application/pdf"));
    expect(host.querySelector("iframe")).toBeNull();
    expect(host.textContent).toContain("workspacePdfHint");
    expect(host.querySelector('a[target="_blank"]')).not.toBeNull();
  } finally {
    vi.unstubAllGlobals();
  }
});

it("uses native audio controls without autoplay and retains download after decoding errors", async () => {
  await render(file("recording.mp3", "audio/mpeg"));
  const audio = host.querySelector("audio")!;
  expect(audio.controls).toBe(true);
  expect(audio.autoplay).toBe(false);
  expect(audio.preload).toBe("metadata");
  await act(async () => audio.dispatchEvent(new Event("error")));
  expect(host.querySelector('[role="alert"]')?.textContent).toBe("workspaceAudioUnavailable");
  expect(host.querySelector("a[download]")).not.toBeNull();
});

it("renders tables by default and preserves CRLF when switching to source, editing and previewing", async () => {
  await render({
    ...file("table.csv", "text/csv"),
    binary: false,
    truncated: false,
    content: 'name,details\r\nAlice,"one, two"\r\n',
  });
  expect(host.querySelector("table")?.textContent).toContain("one, two");
  await act(async () =>
    host.querySelector<HTMLButtonElement>('[aria-label="workspaceViewSource"]')!.click(),
  );
  const editor = host.querySelector("textarea")!;
  await act(async () => {
    Object.getOwnPropertyDescriptor(window.HTMLTextAreaElement.prototype, "value")!.set!.call(
      editor,
      'name,details\nAlice,"changed, text"\n',
    );
    editor.dispatchEvent(new Event("input", { bubbles: true }));
  });
  await act(async () =>
    host.querySelector<HTMLButtonElement>('[aria-label="workspacePreview"]')!.click(),
  );
  expect(host.querySelector("table")?.textContent).toContain("changed, text");
  await act(async () =>
    host.querySelector<HTMLButtonElement>('[aria-label="workspaceViewSource"]')!.click(),
  );
  expect(host.querySelector("textarea")?.value).toContain("changed, text");
});

it.each([
  ["drawing.png", "image/png", "img"],
  ["clip.mp4", "video/mp4", "video"],
  ["report.pdf", "application/pdf", "iframe"],
])(
  "requests fresh %s contents on explicit reload even when metadata has not changed",
  async (path, mime, selector) => {
    await render(file(path, mime));
    const original = host.querySelector(selector)!.getAttribute("src");
    await act(async () => host.querySelector<HTMLButtonElement>('[title="Reload file"]')!.click());
    const updated = host.querySelector(selector)!.getAttribute("src");
    expect(updated).not.toBe(original);
    expect(new URL(updated!, location.href).searchParams.get("reload")).toBe("1");
  },
);
