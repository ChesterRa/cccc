// @vitest-environment happy-dom
import { act } from "react";
import { createRoot } from "react-dom/client";
import { describe, expect, it, vi } from "vite-plus/test";
import { CodexVoiceMessageSources } from "./CodexVoiceMessageSources";

vi.mock("react-i18next", () => ({ useTranslation: () => ({ t: (key: string) => key }) }));
vi.mock("../../services/api/codexVoice", () => ({
  fetchVoiceNotifications: vi.fn(async () => ({
    ok: true,
    result: {
      pending_count: 0,
      unconfirmed_count: 1,
      suppressed_count: 1,
      messages: [
        {
          source: { group_id: "fixture", event_id: "skipped" },
          by: "worker",
          output_status: "suppressed",
          suppression_reason: "viewed",
        },
        {
          source: { group_id: "fixture", event_id: "result" },
          by: "worker",
          output_status: "unconfirmed",
        },
      ],
    },
  })),
}));

(globalThis as typeof globalThis & { IS_REACT_ACT_ENVIRONMENT: boolean }).IS_REACT_ACT_ENVIRONMENT =
  true;

describe("Voice notification delivery status", () => {
  it("shows known queued speech separately from delivery uncertainty and preserves source access", async () => {
    const host = document.createElement("div");
    const root = createRoot(host);
    const open = vi.fn();
    await act(async () =>
      root.render(
        <CodexVoiceMessageSources
          active
          onOpenSource={open}
          outputStatus={{ queued: 1, blocked: "conversation" }}
        />,
      ),
    );
    expect(host.querySelector("summary")?.textContent).toContain("voicePreferences.queued");
    expect(host.querySelector("summary")?.textContent).toContain("voicePreferences.suppressed");
    expect(host.textContent).toContain("voicePreferences.suppressed_viewed");
    expect(host.textContent).toContain("voicePreferences.status_unconfirmed");
    expect(host.textContent).not.toContain("status_undefined");
    expect(host.querySelector("summary")?.textContent).not.toContain(
      "voicePreferences.unconfirmed",
    );
    expect(host.querySelector('[role="status"]')?.textContent).toContain(
      "voicePreferences.wait_conversation",
    );
    expect(host.querySelector('[role="status"]')?.textContent).toContain(
      "voicePreferences.unconfirmed",
    );
    host.querySelector("button")?.click();
    expect(open).toHaveBeenCalledWith("fixture", "result");
    await act(async () =>
      root.render(<CodexVoiceMessageSources active outputStatus={{ queued: 0, blocked: null }} />),
    );
    expect(host.querySelector("summary")?.textContent).toContain("voicePreferences.unconfirmed");
    expect(host.querySelector('[role="status"]')).toBeNull();
    await act(async () => root.unmount());
  });
});
