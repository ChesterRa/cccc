// @vitest-environment happy-dom

import type { TFunction } from "i18next";
import { act } from "react";
import { createRoot } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, it, vi } from "vite-plus/test";

import { VoiceArchiveDialog } from "./VoiceLibraryDialogs";

vi.mock("../../../components/document/MarkdownDocumentSurface", () => ({
  MarkdownDocumentSurface: ({ content }: { content: string }) => <p>{content}</p>,
}));

const t = ((key: string, options?: { defaultValue?: string }) =>
  options?.defaultValue || key) as TFunction;

describe("VoiceArchiveDialog", () => {
  let host: HTMLDivElement;
  let root: ReturnType<typeof createRoot>;

  beforeEach(() => {
    (globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;
    host = document.createElement("div");
    document.body.append(host);
    root = createRoot(host);
  });

  afterEach(async () => {
    await act(async () => root.unmount());
    host.remove();
  });

  it("focuses the opened preview, including when the same document is opened again", async () => {
    await act(async () =>
      root.render(
        <VoiceArchiveDialog
          documents={[
            { document_id: "one", title: "First note", status: "archived", content: "Text" },
            { document_id: "two", title: "Second note", status: "archived", content: "More" },
          ]}
          busy={false}
          isDark={false}
          onClose={vi.fn()}
          onRestore={vi.fn()}
          onDelete={vi.fn()}
          t={t}
        />,
      ),
    );
    const button = [...document.querySelectorAll("button")].find(
      (element) => element.textContent === "First note",
    );
    expect(button).toBeDefined();

    await act(async () => button?.click());
    const preview = document.querySelector('section[aria-label="First note"]');
    expect(preview?.textContent).toContain("Text");
    expect(document.activeElement).toBe(preview);

    button?.focus();
    await act(async () => button?.click());
    expect(document.activeElement).toBe(preview);
  });
});
