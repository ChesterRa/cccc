// @vitest-environment happy-dom
import { act } from "react";
import { createRoot } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, it, vi } from "vite-plus/test";
import { VoicePanelInputBar } from "./VoicePanelInputBar";

describe("VoicePanelInputBar", () => {
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

  const render = (props: Partial<Parameters<typeof VoicePanelInputBar>[0]> = {}) =>
    act(async () =>
      root.render(
        <VoicePanelInputBar
          isDark={false}
          recordLabel="Record into this document"
          recordDisabled={false}
          recording={false}
          recordingStarting={false}
          recordingTargetLabel="Recording into Notes"
          stopLabel="Stop & save"
          liveSnippet=""
          onToggleRecord={() => undefined}
          value="Summarize"
          onChange={() => undefined}
          placeholder="Ask"
          inputLabel="Request"
          actions={[]}
          {...props}
        />,
      ),
    );

  it("shows the pinned target and stop control while any recording is active", async () => {
    const toggle = vi.fn();
    await render({
      recording: true,
      recordDisabled: true,
      liveSnippet: "first words",
      onToggleRecord: toggle,
    });
    const record = host.querySelector<HTMLButtonElement>("[data-voice-record]")!;
    expect(host.querySelector("[data-voice-input-recording]")?.textContent).toContain(
      "Recording into Notes",
    );
    expect(host.textContent).toContain("first words");
    expect(record.textContent).toBe("Stop & save");
    expect(record.disabled).toBe(false);
    expect(record.getAttribute("aria-pressed")).toBe("true");
    await act(async () => record.click());
    expect(toggle).toHaveBeenCalledTimes(1);
  });

  it("submits the enabled primary action with Ctrl+Enter only", async () => {
    const update = vi.fn();
    const ask = vi.fn();
    await render({
      actions: [
        { key: "ask", label: "Ask", onClick: ask },
        { key: "update", label: "Update", onClick: update, primary: true },
      ],
    });
    const field = host.querySelector("textarea")!;
    await act(async () => {
      field.dispatchEvent(new KeyboardEvent("keydown", { key: "Enter", bubbles: true }));
    });
    expect(update).not.toHaveBeenCalled();
    await act(async () => {
      field.dispatchEvent(
        new KeyboardEvent("keydown", { key: "Enter", ctrlKey: true, bubbles: true }),
      );
    });
    expect(update).toHaveBeenCalledTimes(1);
    expect(ask).not.toHaveBeenCalled();
  });
});
