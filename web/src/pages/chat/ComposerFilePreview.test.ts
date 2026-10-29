// @vitest-environment happy-dom
import { act, createElement, StrictMode } from "react";
import { createRoot } from "react-dom/client";
import { ComposerFilePreview } from "./ComposerFilePreview";
import { describe, expect, it, vi } from "vite-plus/test";

import {
  getComposerPreviewPosition,
  isPreviewableComposerImageFile,
} from "./ComposerFilePreview.model";

describe("isPreviewableComposerImageFile", () => {
  it("only enables hover previews for image files", () => {
    expect(
      isPreviewableComposerImageFile(new File(["png"], "image.png", { type: "image/png" })),
    ).toBe(true);
    expect(
      isPreviewableComposerImageFile(new File(["txt"], "notes.txt", { type: "text/plain" })),
    ).toBe(false);
  });
});

describe("getComposerPreviewPosition", () => {
  it("keeps the floating image preview inside the viewport", () => {
    const position = getComposerPreviewPosition({
      anchor: { left: 90, right: 260, top: 460, bottom: 492, width: 170, height: 32 },
      viewport: { width: 360, height: 520 },
      preview: { width: 224, height: 230 },
      gap: 8,
      margin: 12,
    });

    expect(position.left).toBeGreaterThanOrEqual(12);
    expect(position.left + 224).toBeLessThanOrEqual(348);
    expect(position.top).toBeGreaterThanOrEqual(12);
    expect(position.top + 230).toBeLessThanOrEqual(508);
  });
});

describe("composer attachment previews", () => {
  it("shows a thumbnail before hover, falls back on decode errors and preserves removal", async () => {
    vi.stubGlobal("IS_REACT_ACT_ENVIRONMENT", true);
    const create = vi.fn(() => "blob:preview");
    const revoke = vi.fn();
    const NativeURL = URL;
    vi.stubGlobal(
      "URL",
      class extends NativeURL {
        static createObjectURL = create;
        static revokeObjectURL = revoke;
      },
    );
    const host = document.createElement("div");
    document.body.append(host);
    const root = createRoot(host),
      remove = vi.fn();
    const file = new File(["image bytes"], "photo.png", { type: "image/png" });
    try {
      await act(async () =>
        root.render(
          createElement(ComposerFilePreview, {
            file,
            onRemove: remove,
            removeLabel: "Remove photo",
          }),
        ),
      );
      const image = host.querySelector("img")!;
      expect(image.getAttribute("src")).toBe("blob:preview");
      expect(image.alt).toBe("photo.png");
      expect(host.textContent).not.toContain("photo.png");
      await act(async () => image.dispatchEvent(new Event("error")));
      expect(host.querySelector("img")).toBeNull();
      expect(host.textContent).toContain("photo.png");
      await act(async () =>
        host.querySelector<HTMLButtonElement>('button[aria-label="Remove photo"]')!.click(),
      );
      expect(remove).toHaveBeenCalledOnce();
      const documentFile = new File(["notes"], "notes.txt", { type: "text/plain" });
      await act(async () =>
        root.render(
          createElement(ComposerFilePreview, {
            file: documentFile,
            onRemove: remove,
            removeLabel: "Remove notes",
          }),
        ),
      );
      expect(host.textContent).toContain("notes.txt");
      expect(create).toHaveBeenCalledOnce();
      expect(revoke).toHaveBeenCalledWith("blob:preview");
    } finally {
      await act(async () => root.unmount());
      host.remove();
      vi.unstubAllGlobals();
    }
  });

  it("releases every allocated URL across StrictMode replay and file replacement", async () => {
    vi.stubGlobal("IS_REACT_ACT_ENVIRONMENT", true);
    let sequence = 0;
    const create = vi.fn(() => `blob:preview-${sequence++}`),
      revoke = vi.fn();
    const NativeURL = URL;
    vi.stubGlobal(
      "URL",
      class extends NativeURL {
        static createObjectURL = create;
        static revokeObjectURL = revoke;
      },
    );
    const host = document.createElement("div");
    const root = createRoot(host);
    try {
      for (const name of ["first.png", "second.png"]) {
        const file = new File(["image bytes"], name, { type: "image/png" });
        await act(async () =>
          root.render(
            createElement(
              StrictMode,
              null,
              createElement(ComposerFilePreview, {
                file,
                onRemove: () => undefined,
                removeLabel: "Remove",
              }),
            ),
          ),
        );
        expect(host.querySelector("img")?.alt).toBe(name);
      }
      await act(async () => root.render(null));
      expect(revoke.mock.calls.map(([url]) => url)).toEqual(
        create.mock.results.map((result) => result.value),
      );
    } finally {
      await act(async () => root.unmount());
      vi.unstubAllGlobals();
    }
  });
});
