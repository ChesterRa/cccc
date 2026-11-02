// @vitest-environment happy-dom

import { act, type ReactNode } from "react";
import { createRoot } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, it, vi } from "vite-plus/test";
import { VoiceWorkspaceFrame } from "./VoiceWorkspaceFrame";

const beam = vi.hoisted(() => ({
  current: null as null | {
    active: boolean;
    processing: boolean;
    strength?: number;
    coreLight?: number;
    onLevel?: (level: number) => void;
  },
}));

vi.mock("voice-glow", () => ({
  VoiceBeam: (props: NonNullable<typeof beam.current> & { children: ReactNode }) => {
    beam.current = props;
    return <div>{props.children}</div>;
  },
}));

describe("VoiceWorkspaceFrame", () => {
  let host: HTMLDivElement;
  let root: ReturnType<typeof createRoot>;
  const level = () => 0.5;
  const frame = (recording: boolean, processing: boolean) => (
    <VoiceWorkspaceFrame recording={recording} processing={processing} level={level} isDark={false}>
      <p>Recording status</p>
    </VoiceWorkspaceFrame>
  );

  beforeEach(() => {
    (globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;
    host = document.createElement("div");
    document.body.append(host);
    root = createRoot(host);
    beam.current = null;
  });

  afterEach(async () => {
    await act(async () => root.unmount());
    host.remove();
  });

  it("shows the existing smoothed audio level in LCD segments and keeps the meter out of the accessibility tree", async () => {
    await act(async () => root.render(frame(true, false)));
    const meter = host.querySelector<HTMLElement>('[data-voice-workspace-meter="recording"]');
    expect(meter?.getAttribute("aria-hidden")).toBe("true");
    expect(beam.current?.active).toBe(true);
    expect(beam.current?.strength).toBe(0);
    expect(beam.current?.coreLight).toBe(0);
    expect(beam.current?.onLevel).toBeTypeOf("function");
    // The level lights whole segments, 16 of them, from the left.
    const lit = () => (meter?.firstElementChild as HTMLElement | null)?.style.clipPath;
    beam.current?.onLevel?.(0.75);
    expect(lit()).toBe("inset(0 25% 0 0)");
    beam.current?.onLevel?.(0.02);
    expect(lit()).toBe("inset(0 100% 0 0)");
    beam.current?.onLevel?.(2);
    expect(lit()).toBe("inset(0 0% 0 0)");
    expect(host.textContent).toContain("Recording status");
  });

  it("stops the recording meter during analysis and removes it when work ends", async () => {
    await act(async () => root.render(frame(true, false)));
    await act(async () => root.render(frame(false, true)));
    expect(host.querySelector('[data-voice-workspace-meter="recording"]')).toBeNull();
    expect(host.querySelector('[data-voice-workspace-meter="processing"]')).not.toBeNull();
    expect(beam.current?.onLevel).toBeUndefined();
    expect(beam.current?.active).toBe(true);
    expect(beam.current?.processing).toBe(true);

    await act(async () => root.render(frame(false, false)));
    expect(host.querySelector("[data-voice-workspace-meter]")).toBeNull();
    expect(beam.current?.active).toBe(false);
    expect(beam.current?.strength).toBe(0);
    expect(beam.current?.coreLight).toBe(0);
    expect(beam.current?.onLevel).toBeUndefined();
  });
});
