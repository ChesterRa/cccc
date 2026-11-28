// @vitest-environment happy-dom
import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, it } from "vite-plus/test";
import { useComposerStore, useUIStore } from "../stores";
import { useDragDrop } from "./useDragDrop";

(globalThis as typeof globalThis & { IS_REACT_ACT_ENVIRONMENT: boolean }).IS_REACT_ACT_ENVIRONMENT =
  true;

let appendFiles: ((files: File[]) => void) | null = null;

function Harness({ groupId }: { groupId: string }) {
  const { handleAppendComposerFiles } = useDragDrop({ selectedGroupId: groupId });
  appendFiles = handleAppendComposerFiles;
  return null;
}

describe("useDragDrop returns from terminals view when files are accepted", () => {
  let host: HTMLDivElement;
  let root: Root;

  beforeEach(async () => {
    appendFiles = null;
    host = document.createElement("div");
    document.body.append(host);
    root = createRoot(host);
    useComposerStore.setState({
      activeGroupId: "g1",
      composerText: "existing draft",
      composerFiles: [],
    });
    useUIStore.getState().setGroupWorkView("g1", "terminals");
    await act(async () => {
      root.render(<Harness groupId="g1" />);
    });
  });

  afterEach(async () => {
    await act(async () => root.unmount());
    host.remove();
    useUIStore.getState().setGroupWorkView("g1", "messages");
    useComposerStore.setState({ composerFiles: [], composerText: "" });
  });

  it("switches to messages and keeps the draft when a valid file is attached", async () => {
    const file = new File(["hello"], "report.txt", { type: "text/plain" });
    await act(async () => {
      appendFiles!([file]);
    });
    expect(useUIStore.getState().chatSessions.g1?.workView).toBe("messages");
    expect(useComposerStore.getState().composerText).toBe("existing draft");
    expect(useComposerStore.getState().composerFiles).toHaveLength(1);
  });

  it("stays in terminals view when every dropped file is rejected as oversized", async () => {
    const oversized = { name: "huge.bin", size: 101 * 1024 * 1024 } as File;
    await act(async () => {
      appendFiles!([oversized]);
    });
    expect(useUIStore.getState().chatSessions.g1?.workView).toBe("terminals");
    expect(useComposerStore.getState().composerFiles).toHaveLength(0);
  });
});
