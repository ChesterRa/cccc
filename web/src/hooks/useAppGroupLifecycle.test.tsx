// @vitest-environment happy-dom
import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, it } from "vite-plus/test";
import { useComposerStore } from "../stores";
import { shouldResetDestGroupForLifecycle, useAppGroupLifecycle } from "./useAppGroupLifecycle";

describe("useAppGroupLifecycle destination group reset", () => {
  it("does not reset an explicit cross-group destination during normal composing", () => {
    expect(
      shouldResetDestGroupForLifecycle({
        selectedGroupId: "g-current",
        destGroupId: "g-target",
        sendGroupId: "g-target",
        hasReplyTarget: false,
      }),
    ).toBe(false);
  });

  it("resets a cross-group destination when a reply pins the selected Group", () => {
    expect(
      shouldResetDestGroupForLifecycle({
        selectedGroupId: "g-current",
        destGroupId: "g-target",
        sendGroupId: "g-target",
        hasReplyTarget: true,
      }),
    ).toBe(true);
  });
});

// The lifecycle effect wired as App wires it, from the real composer store.
Object.assign(globalThis, { IS_REACT_ACT_ENVIRONMENT: true });
let root: Root;
let host: HTMLDivElement;
const noop = () => undefined;
function Harness() {
  const destGroupId = useComposerStore((s) => s.destGroupId);
  const replyTarget = useComposerStore((s) => s.replyTarget);
  const setDestGroupId = useComposerStore((s) => s.setDestGroupId);
  useAppGroupLifecycle({
    selectedGroupId: "g-a",
    destGroupId,
    sendGroupId: destGroupId,
    hasReplyTarget: !!replyTarget,
    setDestGroupId,
    fileInputRef: { current: null },
    resetDragDrop: noop,
    setActiveTab: noop,
    closeChatWindow: noop,
    loadGroup: noop,
    connectStream: noop,
    cleanupSSE: noop,
  });
  return null;
}

describe("useAppGroupLifecycle with composer attachments", () => {
  beforeEach(async () => {
    useComposerStore.setState({ ...useComposerStore.getInitialState() });
    useComposerStore.getState().switchGroup(null, "g-a");
    host = document.createElement("div");
    document.body.append(host);
    root = createRoot(host);
    await act(async () => root.render(<Harness />));
  });
  afterEach(async () => {
    await act(async () => root.unmount());
    host.remove();
  });

  const file = new File(["png"], "chart.png", { type: "image/png" });
  it.each([
    ["choosing the Group first", ["dest", "files"]],
    ["attaching the file first", ["files", "dest"]],
  ])("keeps another Group as the destination when %s", async (_, order) => {
    for (const step of order) {
      await act(async () => {
        if (step === "dest") useComposerStore.getState().setDestGroupId("g-b");
        else useComposerStore.getState().setComposerFiles([file]);
      });
    }
    expect(useComposerStore.getState().destGroupId).toBe("g-b");
    expect(useComposerStore.getState().composerFiles).toEqual([file]);
  });
});
