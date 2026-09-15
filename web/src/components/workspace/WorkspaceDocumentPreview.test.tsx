// @vitest-environment happy-dom
import { act } from "react";
import { createRoot } from "react-dom/client";
import { expect, it, vi } from "vite-plus/test";
import { WorkspaceDocumentPreview } from "./WorkspaceDocumentPreview";

vi.mock("react-i18next", () => ({ useTranslation: () => ({ t: (key: string) => key }) }));
it("resolves Markdown images and opens relative file links under the original scope, leaving external links separate", async () => {
  Object.assign(globalThis, { IS_REACT_ACT_ENVIRONMENT: true });
  const host = document.createElement("div");
  document.body.append(host);
  const root = createRoot(host);
  const onOpenFile = vi.fn();
  const open = vi.spyOn(window, "open").mockReturnValue(null);
  const file = {
    scope_key: "scope-a",
    scope_url: "/repo",
    path: "docs/readme.md",
    content: "",
    mime_type: "text/markdown",
    binary: false,
    truncated: false,
    sha256: "old",
    bytes: 100,
  };
  try {
    await act(async () => {
      await import("../MarkdownRenderer");
      root.render(
        <WorkspaceDocumentPreview
          groupId="g_a"
          file={file}
          kind="markdown"
          content={
            "# Report\n\n![Figure](../image.svg)\n\n[Next](next.md) [External](https://example.com)"
          }
          isDark={false}
          onOpenFile={onOpenFile}
        />,
      );
    });
    expect(host.querySelector("h1")?.textContent).toBe("Report");
    expect(host.querySelector("h1")?.id).toBe("report");
    const url = new URL(host.querySelector("img")!.src);
    expect(url.searchParams.get("path")).toBe("image.svg");
    expect(url.searchParams.get("scope_key")).toBe("scope-a");
    expect(url.searchParams.get("scope_url")).toBe("/repo");
    await act(async () => host.querySelector("a")!.click());
    expect(onOpenFile).toHaveBeenCalledWith("docs/next.md");
    await act(async () => host.querySelectorAll("a")[1].click());
    expect(open).toHaveBeenCalledWith("https://example.com/", "_blank", "noopener,noreferrer");
  } finally {
    await act(async () => root.unmount());
    host.remove();
    open.mockRestore();
  }
});
