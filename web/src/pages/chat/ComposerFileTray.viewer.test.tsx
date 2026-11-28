// @vitest-environment happy-dom
import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, it, vi } from "vite-plus/test";

import { AttachmentLightbox } from "../../components/messageBubble/AttachmentLightbox";
import { useAttachmentPreviewStore } from "../../stores/useAttachmentPreviewStore";
import { ComposerFileTray } from "./ComposerFileTray";

vi.mock("react-i18next", () => ({ useTranslation: () => ({ t: (key: string) => key }) }));

// The composer's tray together with the page's own attachment viewer, as ChatTab mounts them:
// a waiting picture must open in that viewer, not in one of its own.
describe("pictures waiting in the composer", () => {
  let host: HTMLDivElement;
  let root: Root;
  const first = new File(["a"], "first.png", { type: "image/png" });
  const notes = new File(["b"], "notes.md", { type: "text/markdown" });
  const second = new File(["c"], "second.png", { type: "image/png" });
  const render = (files: File[], groupId = "g1") =>
    act(async () =>
      root.render(
        <>
          <ComposerFileTray
            groupId={groupId}
            files={files}
            onRemove={vi.fn()}
            removeLabel={(file) => `Remove ${file.name}`}
          />
          <AttachmentLightbox groupId={groupId} isDark={false} canOpenInPanel />
        </>,
      ),
    );
  const thumbnail = (name: string) =>
    host.querySelector<HTMLButtonElement>(`button[aria-label="${name}"]`)!;
  const dialog = () => document.querySelector<HTMLElement>('[role="dialog"]');
  const shownSrc = () => dialog()?.querySelector("img")?.getAttribute("src");
  const stepKey = (direction: string) =>
    dialog()!.querySelector<HTMLButtonElement>(`[data-attachment-step="${direction}"]`)!;

  beforeEach(() => {
    Object.assign(globalThis, { IS_REACT_ACT_ENVIRONMENT: true });
    vi.stubGlobal("requestAnimationFrame", (callback: FrameRequestCallback) => {
      callback(0);
      return 1;
    });
    vi.stubGlobal("cancelAnimationFrame", vi.fn());
    // The files exist only in memory; the browser boundary hands back an address for each.
    vi.spyOn(URL, "createObjectURL").mockImplementation(
      (file) => `blob:fixture-${(file as File).name}`,
    );
    vi.spyOn(URL, "revokeObjectURL").mockImplementation(() => undefined);
    host = document.createElement("div");
    document.body.append(host);
    root = createRoot(host);
  });
  afterEach(async () => {
    await act(async () => root.unmount());
    await act(async () => useAttachmentPreviewStore.getState().close());
    host.remove();
    vi.restoreAllMocks();
    vi.unstubAllGlobals();
  });

  it("open in the page's attachment viewer and step through one another there", async () => {
    await render([first, notes, second]);
    expect(dialog()).toBeNull();

    await act(async () => thumbnail("second.png").click());

    expect(dialog()?.textContent).toContain("second.png");
    expect(shownSrc()).toBe("blob:fixture-second.png");
    // Only the pictures take part: the Markdown file between them is skipped.
    expect(stepKey("next").getAttribute("aria-disabled")).toBe("true");
    await act(async () => stepKey("previous").click());
    expect(dialog()?.textContent).toContain("first.png");
    expect(shownSrc()).toBe("blob:fixture-first.png");
    expect(stepKey("previous").getAttribute("aria-disabled")).toBe("true");
  });

  it("offer neither a download nor a move to the panel while they are only local files", async () => {
    await render([first]);
    await act(async () => thumbnail("first.png").click());

    expect(dialog()).not.toBeNull();
    expect(dialog()!.querySelector("a[download], [data-attachment-open-in-panel]")).toBeNull();
  });

  it("leave the viewer when the picture it shows is removed from the draft", async () => {
    await render([first, second]);
    await act(async () => thumbnail("second.png").click());
    expect(shownSrc()).toBe("blob:fixture-second.png");

    await render([first]);

    expect(dialog()).toBeNull();
    expect(useAttachmentPreviewStore.getState().viewing).toBeNull();
    expect(URL.revokeObjectURL).toHaveBeenCalledWith("blob:fixture-second.png");
  });

  it("show a file that is not a picture by name, with nothing to enlarge", async () => {
    await render([notes]);

    expect(host.querySelector('[aria-haspopup="dialog"]')).toBeNull();
    expect(host.textContent).toContain("notes.md");
    expect(URL.createObjectURL).not.toHaveBeenCalled();
  });
});
