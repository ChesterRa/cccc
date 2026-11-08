// @vitest-environment happy-dom
import { act, useEffect, useState } from "react";
import { createRoot } from "react-dom/client";
import { afterEach, expect, it, vi } from "vite-plus/test";
import { ErrorBoundary } from "../ErrorBoundary";
import { PdfPreview } from "./PdfPreview";

vi.mock("react-i18next", () => ({ useTranslation: () => ({ t: (key: string) => key }) }));
vi.mock("../../i18n", () => ({ default: { t: (key: string) => key } }));
vi.mock("./PdfCanvasViewer", async () => {
  throw new Error("fictional PDF reader module failure");
});

afterEach(() => {
  vi.restoreAllMocks();
  vi.unstubAllGlobals();
});

it("keeps the surrounding draft mounted when the PDF reader module cannot load", async () => {
  Object.assign(globalThis, { IS_REACT_ACT_ENVIRONMENT: true });
  vi.stubGlobal("matchMedia", () => ({
    matches: true,
    addEventListener: () => undefined,
    removeEventListener: () => undefined,
  }));
  vi.spyOn(console, "error").mockImplementation(() => {});
  const unmounted = vi.fn();
  function Page() {
    const [draft, setDraft] = useState("unsaved fixture draft");
    useEffect(() => () => unmounted(), []);
    return (
      <>
        <textarea aria-label="Draft" value={draft} readOnly />
        <button onClick={() => setDraft("draft edited after PDF failure")}>Edit draft</button>
        <PdfPreview href="/fixture.pdf" title="Fixture PDF" failedText="Cannot open PDF" />
      </>
    );
  }
  const host = document.createElement("div");
  document.body.append(host);
  const root = createRoot(host);
  try {
    await act(async () => {
      root.render(
        <ErrorBoundary fallback={<p>Whole page failed</p>}>
          <Page />
        </ErrorBoundary>,
      );
    });
    expect(host.querySelector('[role="alert"]')?.textContent).toBe("Cannot open PDF");
    expect(host.textContent).not.toContain("Whole page failed");
    expect(unmounted).not.toHaveBeenCalled();
    expect(host.querySelector("textarea")?.value).toBe("unsaved fixture draft");
    await act(async () => host.querySelector("button")?.click());
    expect(host.querySelector("textarea")?.value).toBe("draft edited after PDF failure");
  } finally {
    await act(async () => root.unmount());
    host.remove();
  }
});
