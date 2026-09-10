// @vitest-environment happy-dom
import { act } from "react";
import { createRoot } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, it, vi } from "vite-plus/test";
import type {
  VoiceAsrProvider,
  VoiceAsrProviderConfig,
} from "../../../services/api/voiceAsrProviders";
const mocks = vi.hoisted(() => ({
  fetch: vi.fn(),
  save: vi.fn(),
  probe: vi.fn(),
  t: (key: string) => key,
}));
vi.mock("../../../services/api/voiceAsrProviders", () => ({
  fetchVoiceAsrProviders: mocks.fetch,
  saveVoiceAsrProvider: mocks.save,
  probeVoiceAsrProvider: mocks.probe,
}));
vi.mock("react-i18next", () => ({ useTranslation: () => ({ t: mocks.t }) }));
import { ExternalAsrSettings } from "./ExternalAsrSettings";

function config(provider: VoiceAsrProvider, configured = false): VoiceAsrProviderConfig {
  return {
    provider,
    configured,
    region: "beijing",
    workspace_id: "",
    model: "fun-asr-realtime",
    resource_id: "volc.seedasr.sauc.duration",
    auth_mode: "api_key",
    has_api_key: configured,
    has_app_id: false,
    has_access_token: false,
  };
}

describe("external ASR settings", () => {
  let host: HTMLDivElement;
  let root: ReturnType<typeof createRoot>;
  const configured = vi.fn();
  beforeEach(() => {
    (globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;
    host = document.createElement("div");
    document.body.append(host);
    root = createRoot(host);
    mocks.fetch.mockReset();
    mocks.save.mockReset();
    mocks.probe.mockReset();
    configured.mockReset();
    mocks.fetch.mockResolvedValue({
      ok: true,
      result: { providers: [config("bailian", true), config("volcengine")] },
    });
  });
  afterEach(async () => {
    await act(async () => root.unmount());
    host.remove();
  });
  const render = (provider: VoiceAsrProvider = "bailian") =>
    act(async () =>
      root.render(
        <ExternalAsrSettings
          provider={provider}
          onProviderChange={vi.fn()}
          disabled={false}
          onConfigured={configured}
        />,
      ),
    );
  const button = (key: string) =>
    [...host.querySelectorAll<HTMLButtonElement>("button")].find(
      (e) => e.textContent === `assistants.externalAsr.${key}`,
    )!;
  async function secret(value: string) {
    const input = host.querySelector<HTMLInputElement>('input[type="password"]')!;
    await act(async () => {
      Object.getOwnPropertyDescriptor(HTMLInputElement.prototype, "value")!.set!.call(input, value);
      input.dispatchEvent(new Event("input", { bubbles: true }));
    });
  }
  it("does not echo stored credentials and clears password drafts after saving", async () => {
    await render();
    expect(host.querySelector<HTMLInputElement>('input[type="password"]')!.value).toBe("");
    expect(button("probe").disabled).toBe(false);
    await secret("test-secret-key");
    expect(button("probe").disabled).toBe(true);
    await act(async () => button("probe").click());
    expect(mocks.probe).not.toHaveBeenCalled();
    mocks.save.mockResolvedValue({ ok: true, result: config("bailian", true) });
    await act(async () => button("save").click());
    expect(mocks.save).toHaveBeenCalledWith(
      "bailian",
      expect.objectContaining({ api_key: "test-secret-key", model: "fun-asr-realtime" }),
    );
    expect(mocks.save.mock.calls[0][1]).not.toHaveProperty("configured");
    expect(host.querySelector<HTMLInputElement>('input[type="password"]')!.value).toBe("");
    expect(configured).toHaveBeenCalledOnce();
    expect(button("probe").disabled).toBe(false);
    expect(mocks.probe).not.toHaveBeenCalled();
  });
  it("clears secrets explicitly and disables probes when unconfigured", async () => {
    await render();
    mocks.save.mockResolvedValue({ ok: true, result: config("bailian") });
    await act(async () => button("clear").click());
    expect(mocks.save).toHaveBeenCalledWith(
      "bailian",
      expect.objectContaining({ clear_credentials: true }),
    );
    expect(mocks.save.mock.calls[0][1]).not.toHaveProperty("api_key");
    expect(button("probe").disabled).toBe(true);
  });
  it("drops the previous provider's unsaved secret on provider change", async () => {
    await render();
    await secret("bailian-only-secret");
    await render("volcengine");
    expect(host.querySelector<HTMLInputElement>('input[type="password"]')!.value).toBe("");
    expect(button("probe").disabled).toBe(true);
    expect(mocks.save).not.toHaveBeenCalled();
  });
  it("shows access failure without exposing a credential form", async () => {
    mocks.fetch.mockResolvedValue({
      ok: false,
      error: { message: "administrator access required" },
    });
    await render();
    expect(host.querySelector('[role="alert"]')?.textContent).toContain(
      "administrator access required",
    );
    expect(host.querySelector('input[type="password"]')).toBeNull();
  });
  it("shows a localized resource-not-granted error without upstream details", async () => {
    const code = "external_asr_resource_not_granted";
    await render();
    mocks.probe.mockResolvedValue({ ok: false, error: { code, message: "upstream detail" } });
    await act(async () => button("probe").click());
    expect(host.querySelector('[role="alert"]')?.textContent).toBe(
      "assistants.externalAsr.resourceNotGranted",
    );
    expect(host.textContent).not.toContain("upstream detail");
  });
});
