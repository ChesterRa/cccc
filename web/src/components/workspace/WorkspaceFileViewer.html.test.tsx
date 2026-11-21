// @vitest-environment happy-dom
import type { Window as HappyWindow } from "happy-dom";
import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, expect, it, vi } from "vite-plus/test";
import type { WorkspaceFile } from "../../types";
import { HTML_PREVIEW_BYTES, DOCUMENT_PREVIEW_BYTES } from "../messageBubble/documentText";
import { WorkspaceFileViewer } from "./WorkspaceFileViewer";

vi.mock("react-i18next", () => ({ useTranslation: () => ({ t: (key: string) => key }) }));
let root: Root;
let host: HTMLDivElement;
const file: WorkspaceFile = {
  scope_key: "scope-a",
  scope_url: "/project a",
  path: "slides/deck.html",
  mime_type: "text/html",
  bytes: 3_000_000,
  content: "<h1>Prefix only</h1>",
  binary: false,
  truncated: true,
  sha256: "",
};
const onSave = vi.fn(async () => true);
const render = (value = file, reloadVersion = 0) =>
  act(async () =>
    root.render(
      <WorkspaceFileViewer
        groupId="g_a"
        file={value}
        draft={value.content}
        setDraft={vi.fn()}
        isDark={false}
        readOnly={false}
        saving={false}
        error=""
        conflict={false}
        onClose={vi.fn()}
        onSave={onSave}
        onReload={vi.fn()}
        onAttach={vi.fn()}
        reloadVersion={reloadVersion}
      />,
    ),
  );
beforeEach(() => {
  Object.assign(globalThis, { IS_REACT_ACT_ENVIRONMENT: true });
  (window as unknown as HappyWindow).happyDOM.settings.disableIframePageLoading = true;
  host = document.createElement("div");
  document.body.append(host);
  root = createRoot(host);
  onSave.mockClear();
});
afterEach(async () => {
  await act(async () => root.unmount());
  host.remove();
  vi.unstubAllGlobals();
  (window as unknown as HappyWindow).happyDOM.settings.disableIframePageLoading = false;
});

it("reads complete HTML for preview without making a truncated file editable", async () => {
  const original =
    "<h1>Complete deck</h1><!--" +
    "x".repeat(DOCUMENT_PREVIEW_BYTES) +
    '--><img src="figure.png"><script>bad()</script>';
  vi.stubGlobal(
    "fetch",
    vi.fn(async () => new Response(original)),
  );
  await render();
  const request = new URL(vi.mocked(fetch).mock.calls[0][0] as string, location.href);
  expect(request.searchParams.get("path")).toBe(file.path);
  expect(request.searchParams.get("scope_key")).toBe(file.scope_key);
  expect(request.searchParams.get("scope_url")).toBe(file.scope_url);
  expect(host.querySelector("iframe")?.srcdoc).toContain("Complete deck");
  expect(host.querySelector("iframe")?.srcdoc).toContain("figure.png");
  expect(host.querySelector("iframe")?.srcdoc).not.toContain("<script>");
  expect(host.querySelector("iframe")?.getAttribute("sandbox")).toBe("allow-same-origin");
  await act(async () =>
    host.querySelector<HTMLButtonElement>('[aria-label="workspaceViewSource"]')!.click(),
  );
  expect(host.querySelector("pre")?.textContent).toBe(original.slice(0, DOCUMENT_PREVIEW_BYTES));
  expect(host.querySelector('[role="status"]')?.textContent).toBe("documentPreviewTruncated");
  expect(host.querySelector("textarea, [title=workspaceSave]")).toBeNull();
  expect(onSave).not.toHaveBeenCalled();
  expect(fetch).toHaveBeenCalledOnce();
});

it("keeps over-limit HTML as a bounded source prefix and never renders a partial page", async () => {
  vi.stubGlobal(
    "fetch",
    vi.fn(async () => new Response("x".repeat(HTML_PREVIEW_BYTES + 1))),
  );
  await render();
  expect(host.querySelector("iframe, textarea")).toBeNull();
  expect(host.textContent).toContain("documentHtmlTruncated");
  expect(host.querySelector("pre")?.textContent).toHaveLength(DOCUMENT_PREVIEW_BYTES);
});

it("reports a failed full read without rendering the original truncated prefix", async () => {
  vi.stubGlobal(
    "fetch",
    vi.fn(async () => new Response("gone", { status: 404 })),
  );
  await render();
  expect(host.querySelector("iframe, textarea")).toBeNull();
  expect(host.querySelector('[role="alert"]')?.textContent).toBe("documentPreviewFailed");
});

it("aborts the old read when reloading the same file", async () => {
  const signals: AbortSignal[] = [];
  vi.stubGlobal(
    "fetch",
    vi.fn((_url, options) => {
      signals.push(options.signal);
      return new Promise(() => {});
    }),
  );
  await render();
  await render(file, 1);
  expect(signals).toHaveLength(2);
  expect(signals[0].aborted).toBe(true);
  expect(signals[1].aborted).toBe(false);
  expect(host.querySelector("iframe, textarea")).toBeNull();
});
