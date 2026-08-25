// @vitest-environment happy-dom

import { act } from "react";
import { createRoot } from "react-dom/client";
import { beforeEach, describe, expect, it, vi } from "vite-plus/test";

(globalThis as typeof globalThis & { IS_REACT_ACT_ENVIRONMENT: boolean }).IS_REACT_ACT_ENVIRONMENT =
  true;

const voiceControl = vi.hoisted(() => vi.fn(() => <div data-testid="voice-control" />));

vi.mock("../../../src/pages/chat/VoiceSecretaryComposerControl", () => ({
  VoiceSecretaryComposerControl: voiceControl,
}));

import { LazyVoiceSecretaryComposerControl } from "../../../src/pages/chat/LazyVoiceSecretaryComposerControl";

describe("LazyVoiceSecretaryComposerControl", () => {
  beforeEach(() => voiceControl.mockClear());

  it("does not mount the voice feature until the user activates its launcher", async () => {
    const host = document.createElement("div");
    const root = createRoot(host);
    await act(async () =>
      root.render(
        <LazyVoiceSecretaryComposerControl
          isDark={false}
          selectedGroupId="g-1"
          busy=""
          variant="assistantRow"
        />,
      ),
    );

    expect(voiceControl).not.toHaveBeenCalled();
    const launcher = host.querySelector<HTMLButtonElement>("button");
    expect(launcher).not.toBeNull();
    await act(async () => launcher?.click());
    await vi.waitFor(() => expect(voiceControl).toHaveBeenCalled());
    expect(voiceControl.mock.calls.at(-1)?.[0]).toEqual(
      expect.objectContaining({ initiallyOpen: true }),
    );
    await act(async () => root.unmount());
  });
});
