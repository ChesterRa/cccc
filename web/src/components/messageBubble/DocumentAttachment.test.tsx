// @vitest-environment happy-dom

import type { Window as HappyWindow } from "happy-dom";
import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, it, vi } from "vite-plus/test";

import { DocumentAttachment } from "./DocumentAttachment";
import { DOCUMENT_PREVIEW_BYTES, HTML_PREVIEW_BYTES } from "./documentText";
import { AttachmentResourceView } from "../resourcePanel/views/AttachmentResourceView";
import { MessageLinkPreview } from "./MessageLinkPreview";
import { useResourcePanelStore } from "../../stores/useResourcePanelStore";

vi.mock("react-i18next", () => ({ useTranslation: () => ({ t: (key: string) => key }) }));
vi.mock("../viewer/PdfCanvasViewer", () => ({
  PdfCanvasViewer: ({ href }: { href: string }) => <div data-pdf-canvas-viewer={href} />,
}));
vi.mock("../LazyMarkdownRenderer", () => ({
  LazyMarkdownRenderer: (props: {
    content: string;
    resolveUrl?: (url: string, kind: "image" | "link") => string;
  }) => (
    <div data-rendered-markdown>
      {props.content}
      <a href={props.resolveUrl?.("https://example.test/a", "link")}>outside</a>
      <a href={props.resolveUrl?.("./sibling.md", "link") || undefined}>relative</a>
    </div>
  ),
}));

let root: Root | null = null;

const PREVIEW = "/api/v1/groups/g1/blobs/hash?filename=notes";

async function open(kind: "markdown" | "html" | "text" | "pdf", href = PREVIEW) {
  const host = document.createElement("div");
  document.body.append(host);
  root = createRoot(host);
  await act(async () =>
    root!.render(
      <AttachmentResourceView
        media={kind}
        href={href}
        title="notes"
        isSvg={false}
        isDark={false}
        active
        resourceKey="fixture"
      />,
    ),
  );
  await act(async () => {
    await new Promise((resolve) => setTimeout(resolve));
  });
  return host;
}

function respondWith(body: BodyInit, status = 200, headers?: HeadersInit) {
  vi.stubGlobal(
    "fetch",
    vi.fn(async () => new Response(body, { status, headers })),
  );
}

beforeEach(() => {
  (window as unknown as HappyWindow).happyDOM.settings.disableIframePageLoading = true;
  Object.defineProperty(navigator, "pdfViewerEnabled", { value: true, configurable: true });
});

afterEach(async () => {
  await act(async () => root?.unmount());
  root = null;
  document.body.replaceChildren();
  (window as unknown as HappyWindow).happyDOM.settings.disableIframePageLoading = false;
  vi.unstubAllGlobals();
  vi.restoreAllMocks();
  useResourcePanelStore.setState({ groups: {} });
});

describe("DocumentAttachment", () => {
  it("loads nothing until it is opened, and keeps its download beside it", async () => {
    const fetchMock = vi.fn();
    vi.stubGlobal("fetch", fetchMock);
    const host = document.createElement("div");
    document.body.append(host);
    root = createRoot(host);
    await act(async () =>
      root!.render(
        <DocumentAttachment
          resource={{
            panelGroupId: "g1",
            blobGroupId: "g1",
            messageKey: "event",
            eventId: "event",
            index: 0,
            blobPath: "state/blobs/notes",
          }}
          kind="markdown"
          downloadHref="/download"
          downloadName="notes.md"
          label="notes.md"
          downloadTitle="Download notes.md"
        />,
      ),
    );

    expect(fetchMock).not.toHaveBeenCalled();
    const download = host.querySelector("[data-message-document] a");
    expect(download?.getAttribute("href")).toBe("/download");
    expect(download?.getAttribute("download")).toBe("notes.md");
  });

  it("renders Markdown in the page", async () => {
    respondWith("# Title");
    await open("markdown");

    const rendered = document.querySelector("[data-rendered-markdown]");
    expect(rendered?.textContent).toContain("# Title");
    expect(fetch).toHaveBeenCalledWith(
      PREVIEW,
      expect.objectContaining({ credentials: "same-origin" }),
    );
  });

  it("switches between Markdown and its unchanged source without loading it again", async () => {
    const original = "# Title\n\n[Link](https://example.test)\n\n```js\n  keepIndent();\n```\n";
    respondWith(original);
    await open("markdown");
    await act(async () =>
      document.querySelector<HTMLButtonElement>('[aria-label="workspaceViewSource"]')!.click(),
    );
    expect(document.querySelector("[data-rendered-markdown]")).toBeNull();
    expect(document.querySelector("[data-document-preview] pre")?.textContent).toBe(original);
    await act(async () =>
      document.querySelector<HTMLButtonElement>('[aria-label="workspacePreview"]')!.click(),
    );
    expect(document.querySelector("[data-rendered-markdown]")).not.toBeNull();
    expect(fetch).toHaveBeenCalledTimes(1);
  });

  it("does not carry source mode or old content to a different document", async () => {
    vi.stubGlobal(
      "fetch",
      vi.fn(async (href: string) => new Response(href)),
    );
    await open("markdown");
    await act(async () =>
      document.querySelector<HTMLButtonElement>('[aria-label="workspaceViewSource"]')!.click(),
    );
    await act(async () =>
      root!.render(
        <AttachmentResourceView
          media="markdown"
          href="/second.md"
          title="second"
          isSvg={false}
          isDark={false}
          active
          resourceKey="second"
        />,
      ),
    );
    expect(document.querySelector("[data-rendered-markdown]")?.textContent).toContain("/second.md");
    expect(document.querySelector('[aria-label="workspaceViewSource"]')).not.toBeNull();
    expect(document.querySelector("[data-document-preview] pre")).toBeNull();
  });

  it("keeps each pane's source switch in its own header and removes a hidden tab's switch", async () => {
    respondWith("# Shared document");
    const host = document.createElement("div");
    const left = document.createElement("header");
    const right = document.createElement("header");
    document.body.append(left, right, host);
    root = createRoot(host);
    const render = (leftTarget: HTMLElement | null) => (
      <>
        <AttachmentResourceView
          media="markdown"
          href={PREVIEW}
          title="left"
          isSvg={false}
          isDark={false}
          active
          resourceKey="left"
          actionsTarget={leftTarget}
        />
        <AttachmentResourceView
          media="markdown"
          href={PREVIEW}
          title="right"
          isSvg={false}
          isDark={false}
          active
          resourceKey="right"
          actionsTarget={right}
        />
      </>
    );
    await act(async () => root!.render(render(left)));
    expect(host.querySelector("button")).toBeNull();
    expect(left.querySelector("button")).not.toBeNull();
    expect(right.querySelector("button")).not.toBeNull();
    await act(async () => left.querySelector("button")!.click());
    expect(host.querySelectorAll("pre")).toHaveLength(1);
    expect(host.querySelectorAll("[data-rendered-markdown]")).toHaveLength(1);
    await act(async () => root!.render(render(null)));
    expect(left.querySelector("button")).toBeNull();
    expect(right.querySelector("button")).not.toBeNull();
    expect(host.querySelectorAll("pre")).toHaveLength(1);
    expect(fetch).toHaveBeenCalledTimes(2);
    await act(async () => root!.render(null));
    expect(right.querySelector("button")).toBeNull();
  });

  it("puts a link preview's switch beside its download without another toolbar", async () => {
    respondWith("# Linked document");
    const host = document.createElement("div");
    document.body.append(host);
    root = createRoot(host);
    await act(async () =>
      root!.render(
        <MessageLinkPreview
          preview={{
            kind: "markdown",
            href: PREVIEW,
            label: "notes.md",
            downloadHref: "/download/notes.md",
          }}
          isDark={false}
          onClose={() => {}}
        />,
      ),
    );
    const header = document.querySelector("[data-media-header]")!;
    const toggle = header.querySelector<HTMLButtonElement>('[aria-label="workspaceViewSource"]');
    expect(toggle).not.toBeNull();
    expect(header.querySelector('a[download="notes.md"]')).not.toBeNull();
    expect(document.querySelector("[data-document-preview] button")).toBeNull();
    await act(async () => toggle!.click());
    expect(document.querySelector("[data-document-preview] pre")?.textContent).toBe(
      "# Linked document",
    );
    expect(fetch).toHaveBeenCalledOnce();
  });

  it("previews HTML only through the static sandbox and keeps the original as source", async () => {
    const original =
      '<style>h1{color:red}</style><h1 onclick="alert(1)">Report</h1><script>parent.compromised=true</script><img src="https://outside.test/a.png"><img src="data:image/png;base64,AA=="><a href="javascript:alert(1)">Bad</a>';
    respondWith(original);
    await open("html");
    const frame = document.querySelector("iframe")!;
    expect(frame.getAttribute("src")).toBeNull();
    expect(frame.getAttribute("sandbox")).toBe("allow-same-origin");
    const template = document.createElement("template");
    template.innerHTML = frame.srcdoc;
    expect(template.content.querySelector("h1")?.textContent).toBe("Report");
    expect(template.content.querySelector("script, [onclick]")).toBeNull();
    expect(template.content.querySelector("img")?.getAttribute("src")).toBeNull();
    expect(template.content.querySelectorAll("img")[1].getAttribute("src")).toContain(
      "data:image/png",
    );
    expect(template.content.querySelector("a")?.getAttribute("href")).toBeNull();
    const policy = template.content.querySelector("meta")?.getAttribute("content");
    expect(policy).toContain("img-src data:");
    expect(policy).toContain("media-src 'none'");
    expect(policy).not.toContain("https://outside.test");
    await act(async () =>
      document.querySelector<HTMLButtonElement>('[aria-label="workspaceViewSource"]')!.click(),
    );
    expect(document.querySelector("iframe")).toBeNull();
    expect(document.querySelector("[data-document-preview] pre")?.textContent).toBe(original);
    expect(fetch).toHaveBeenCalledTimes(1);
  });

  it("shows truncated HTML as source only instead of rendering a broken page", async () => {
    respondWith("<h1>Too long</h1>" + "x".repeat(HTML_PREVIEW_BYTES));
    await open("html");
    expect(document.querySelector("iframe")).toBeNull();
    expect(document.querySelector('[role="status"]')?.textContent).toBe("documentHtmlTruncated");
    expect(document.querySelector("[data-document-preview] pre")?.textContent).toHaveLength(
      DOCUMENT_PREVIEW_BYTES,
    );
    expect(document.querySelector('[aria-label="workspacePreview"]')).toBeNull();
    expect(document.querySelector('[aria-label="workspaceViewSource"]')).toBeNull();
  });

  it("renders complete HTML above the text limit and bounds its read-only source view", async () => {
    const original =
      "<h1>Large slide deck</h1><!--" +
      "x".repeat(2 * DOCUMENT_PREVIEW_BYTES) +
      "--><h2>Tail marker</h2>";
    respondWith(original);
    await open("html");
    expect(document.querySelector("iframe")?.srcdoc).toContain("Large slide deck");
    expect(document.querySelector("iframe")?.srcdoc).toContain("Tail marker");
    expect(document.querySelector('[role="status"]')).toBeNull();
    await act(async () =>
      document.querySelector<HTMLButtonElement>('[aria-label="workspaceViewSource"]')!.click(),
    );
    expect(document.querySelector("[data-document-preview] pre")?.textContent).toBe(
      original.slice(0, DOCUMENT_PREVIEW_BYTES),
    );
    expect(document.querySelector('[role="status"]')?.textContent).toBe("documentPreviewTruncated");
    expect(fetch).toHaveBeenCalledOnce();
  });

  it("opens outside links in a new tab and never navigates the page for a relative one", async () => {
    respondWith("text");
    const windowOpen = vi.spyOn(window, "open").mockImplementation(() => null);
    await open("markdown");
    const links = document.querySelectorAll<HTMLAnchorElement>("[data-rendered-markdown] a");

    await act(async () => links[0].click());
    expect(windowOpen).toHaveBeenCalledWith(
      "https://example.test/a",
      "_blank",
      "noopener,noreferrer",
    );

    windowOpen.mockClear();
    expect(links[1].hasAttribute("href")).toBe(false);
    await act(async () => links[1].click());
    expect(windowOpen).not.toHaveBeenCalled();
  });

  it("shows text as it is, without rendering it", async () => {
    respondWith("<b>not bold</b>");
    await open("text");

    const text = document.querySelector("[data-document-preview] pre");
    expect(text?.textContent).toBe("<b>not bold</b>");
    expect(document.querySelector("[data-document-preview] b")).toBeNull();
  });

  it("says when only the start of a long file is shown", async () => {
    respondWith("a".repeat(DOCUMENT_PREVIEW_BYTES + 10));
    await open("text");

    expect(document.querySelector("[data-document-preview] pre")?.textContent).toHaveLength(
      DOCUMENT_PREVIEW_BYTES,
    );
    expect(document.querySelector('[role="status"]')?.textContent).toBe("documentPreviewTruncated");
  });

  it("does not leave half a character where the limit falls", async () => {
    respondWith("a".repeat(DOCUMENT_PREVIEW_BYTES - 1) + "字字");
    await open("text");

    const text = document.querySelector("[data-document-preview] pre")?.textContent || "";
    expect(text.endsWith("�")).toBe(false);
    expect(text).toHaveLength(DOCUMENT_PREVIEW_BYTES - 1);
  });

  it("does not show a binary file as text", async () => {
    respondWith(new Uint8Array([0x25, 0x00, 0x41]));
    await open("markdown");

    expect(document.querySelector("[data-document-preview]")).toBeNull();
    expect(document.body.textContent).toContain("workspaceBinary");
  });

  it("says so when the file cannot be read", async () => {
    respondWith("gone", 404);
    await open("text");

    expect(document.querySelector('[role="alert"]')?.textContent).toBe("documentPreviewFailed");
  });

  it("stops loading when it is closed", async () => {
    let signal: AbortSignal | undefined;
    vi.stubGlobal(
      "fetch",
      vi.fn((_url: string, init: RequestInit) => {
        signal = init.signal ?? undefined;
        return new Promise(() => {});
      }),
    );
    await open("text");
    expect(signal?.aborted).toBe(false);

    await act(async () => root!.render(null));
    expect(signal?.aborted).toBe(true);
  });

  it("gives a PDF to the browser's viewer", async () => {
    respondWith("%PDF-1.7\n", 200, { "content-type": "application/pdf" });
    await open("pdf", "about:blank#notes");

    expect(document.querySelector("iframe")?.getAttribute("src")).toBe("about:blank#notes");
  });

  it("checks a replacement PDF address before placing it in the browser viewer", async () => {
    let finish: ((response: Response) => void) | undefined;
    vi.stubGlobal(
      "fetch",
      vi.fn((url: string) =>
        url === "/first.pdf"
          ? Promise.resolve(
              new Response("%PDF-1.7\n", { headers: { "content-type": "application/pdf" } }),
            )
          : new Promise<Response>((resolve) => {
              finish = resolve;
            }),
      ),
    );
    await open("pdf", "/first.pdf");
    expect(document.querySelector("iframe")?.getAttribute("src")).toBe("/first.pdf");
    await act(async () =>
      root!.render(
        <AttachmentResourceView
          media="pdf"
          href="/second.pdf"
          title="notes"
          isSvg={false}
          isDark={false}
          active
          resourceKey="fixture"
        />,
      ),
    );
    expect(document.querySelector("iframe")).toBeNull();
    await act(async () =>
      finish?.(new Response("not a PDF", { headers: { "content-type": "text/html" } })),
    );
    expect(document.querySelector("iframe")).toBeNull();
    expect(document.querySelector('[role="alert"]')?.textContent).toBe("documentPreviewFailed");
  });

  it.each([
    ["only calls itself a PDF", "<html>not a pdf</html>", { "content-type": "application/pdf" }],
    ["would download", "%PDF-1.7\n", { "content-type": "application/octet-stream" }],
    [
      "is sent as an attachment",
      "%PDF-1.7\n",
      { "content-type": "application/pdf", "content-disposition": 'attachment; filename="a.pdf"' },
    ],
  ])("does not hand the frame a file that %s", async (_name, body, headers) => {
    respondWith(body, 200, headers);
    await open("pdf");

    expect(document.querySelector("iframe")).toBeNull();
    expect(document.querySelector('[role="alert"]')?.textContent).toBe("documentPreviewFailed");
  });

  it("uses the in-page renderer when the browser has no PDF viewer", async () => {
    Object.defineProperty(navigator, "pdfViewerEnabled", { value: false, configurable: true });
    respondWith("%PDF-1.7\n", 200, { "content-type": "application/pdf" });
    await open("pdf");

    expect(document.querySelector("iframe")).toBeNull();
    expect(
      document.querySelector("[data-pdf-canvas-viewer]")?.getAttribute("data-pdf-canvas-viewer"),
    ).toBe(PREVIEW);
  });
});
