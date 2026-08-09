// @vitest-environment happy-dom

import { act, createElement } from "react";
import { createRoot } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, it, vi } from "vite-plus/test";

import enChat from "../../src/i18n/locales/en/chat.json";
import jaChat from "../../src/i18n/locales/ja/chat.json";
import zhChat from "../../src/i18n/locales/zh/chat.json";

const apiMock = vi.hoisted(() => ({
  fetchConnectors: vi.fn(),
  fetchSession: vi.fn(),
  updatePreference: vi.fn(),
}));
const copyMock = vi.hoisted(() => vi.fn());
const surfaceMock = vi.hoisted(() => vi.fn());

vi.mock("react-i18next", () => ({ useTranslation: () => ({ t: (key: string) => key }) }));

vi.mock("../../src/services/api", () => ({
  fetchWebModelConnectors: apiMock.fetchConnectors,
  fetchWebModelBrowserSession: apiMock.fetchSession,
  updateWebModelDeliveryPreference: apiMock.updatePreference,
  getWebModelBrowserSurfaceWebSocketUrl: () => "ws://example.test/browser",
}));

vi.mock("../../src/utils/copy", () => ({ copyTextToClipboard: copyMock }));

vi.mock("../../src/stores", () => ({
  useModalStore: (selector: (state: { openSettingsTarget: () => void }) => unknown) =>
    selector({ openSettingsTarget: vi.fn() }),
}));

vi.mock("../../src/components/browser/ProjectedBrowserSurfacePanel", () => ({
  ProjectedBrowserSurfacePanel: (props: unknown) => {
    surfaceMock(props);
    return null;
  },
}));

import { WebModelRuntimePanel } from "../../src/components/webModel/WebModelRuntimePanel";

describe("WebModelRuntimePanel delivery mode", () => {
  let host: HTMLDivElement;
  let root: ReturnType<typeof createRoot>;

  beforeEach(() => {
    apiMock.fetchSession.mockReset();
    apiMock.fetchConnectors.mockReset();
    apiMock.updatePreference.mockReset();
    copyMock.mockReset();
    surfaceMock.mockReset();
    apiMock.fetchConnectors.mockResolvedValue({ ok: true, result: { connectors: [] } });
    apiMock.fetchSession.mockResolvedValue({
      ok: true,
      result: {
        browser_session: {
          ready: true,
          conversation_url: "https://chatgpt.com/c/test",
          delivery_mode: "standard",
        },
      },
    });
    host = document.createElement("div");
    document.body.append(host);
    root = createRoot(host);
  });

  afterEach(async () => {
    await act(async () => root.unmount());
    host.remove();
    vi.restoreAllMocks();
  });

  async function renderPanel(readOnly = false, isRunning = true) {
    await act(async () => {
      root.render(
        createElement(WebModelRuntimePanel, {
          groupId: "group-1",
          actor: { id: "web-1" },
          isRunning,
          isDark: false,
          isVisible: true,
          readOnly,
        }),
      );
    });
    await vi.waitFor(() => expect(apiMock.fetchSession).toHaveBeenCalledTimes(1));
    await vi.waitFor(() => {
      expect(host.querySelector<HTMLInputElement>('input[value="standard"]')?.checked).toBe(true);
    });
  }

  it("persists a radio selection and renders the daemon-confirmed mode", async () => {
    await renderPanel();
    apiMock.updatePreference.mockResolvedValue({
      ok: true,
      result: { browser_session: { delivery_mode: "image_compat" } },
    });

    const compatibility = host.querySelector<HTMLInputElement>('input[value="image_compat"]')!;
    await act(async () => compatibility.click());

    expect(apiMock.updatePreference).toHaveBeenCalledWith({
      groupId: "group-1",
      actorId: "web-1",
      mode: "image_compat",
    });
    expect(compatibility.checked).toBe(true);
    expect(compatibility.getAttribute("aria-describedby")).toContain(
      "web-model-delivery-mode-image_compat-description",
    );
  });

  it("keeps delivery controls compact while retaining accessible help", async () => {
    await renderPanel();

    const fieldset = host.querySelector("fieldset")!;
    const legend = fieldset.querySelector("legend")!;
    const help = host.querySelector<HTMLButtonElement>(
      'button[aria-label="webModelDelivery.modeHelp"]',
    );

    expect(legend.classList.contains("sr-only")).toBe(true);
    expect(fieldset.className).not.toContain("border-t");
    expect(fieldset.querySelectorAll('input[type="radio"]')).toHaveLength(2);
    expect(help).not.toBeNull();
    expect(host.querySelector("#web-model-delivery-mode-scope")?.classList).toContain("sr-only");
    const selected =
      fieldset.querySelector<HTMLInputElement>('input[value="standard"]')?.nextElementSibling;
    expect(selected?.className).toContain("bg-[var(--glass-tab-bg-active)]");
    expect(selected?.className).not.toContain("bg-[rgb(35,36,37)]");
  });

  it("keeps the server mode and announces an API failure", async () => {
    await renderPanel();
    apiMock.updatePreference.mockResolvedValue({
      ok: false,
      error: { code: "save_failed", message: "Preference was not saved" },
    });

    await act(async () => {
      host.querySelector<HTMLInputElement>('input[value="image_compat"]')!.click();
    });

    expect(host.querySelector<HTMLInputElement>('input[value="standard"]')?.checked).toBe(true);
    expect(host.querySelector('[role="alert"]')?.textContent).toContain("Preference was not saved");
  });

  it("disables actor-scoped controls in read-only mode", async () => {
    await renderPanel(true);

    expect(host.querySelector<HTMLFieldSetElement>("fieldset")?.disabled).toBe(true);
    expect(apiMock.updatePreference).not.toHaveBeenCalled();
  });

  it("copies the actor-bound MCP URL directly from the runtime panel", async () => {
    const mcpUrl = "https://cccc.example/mcp/web-model/wmc_test?token=secret";
    apiMock.fetchConnectors.mockResolvedValue({
      ok: true,
      result: {
        connectors: [
          {
            connector_id: "wmc_test",
            group_id: "group-1",
            actor_id: "web-1",
            connector_url_with_token: mcpUrl,
          },
        ],
      },
    });
    copyMock.mockResolvedValue(true);

    await renderPanel();
    await vi.waitFor(() => expect(host.textContent).toContain("webModelDelivery.copyMcpUrl"));
    const copyButton = host.querySelector<HTMLButtonElement>(
      'button[aria-label="webModelDelivery.copyMcpUrl"]',
    );
    await act(async () => copyButton?.click());

    expect(copyMock).toHaveBeenCalledWith(mcpUrl);
    expect(host.textContent).toContain("webModelDelivery.mcpCopied");
  });

  it("does not open ChatGPT or expose a usable MCP action while the actor is stopped", async () => {
    await renderPanel(false, false);

    expect(surfaceMock).not.toHaveBeenCalled();
    expect(host.textContent).toContain("webModelDelivery.actorStoppedSurface");
    const mcpButton = host.querySelector<HTMLButtonElement>(
      'button[aria-label="webModelDelivery.mcpStartFirstHint"]',
    );
    expect(mcpButton?.disabled).toBe(true);
  });

  it("refreshes delivery status without triggering an inspect mutation", async () => {
    let poll: (() => void) | null = null;
    vi.spyOn(window, "setInterval").mockImplementation((handler) => {
      poll = handler as () => void;
      return 1;
    });
    await renderPanel();
    apiMock.fetchSession.mockResolvedValue({
      ok: true,
      result: {
        browser_session: {
          ready: true,
          conversation_url: "https://chatgpt.com/c/test",
          delivery_mode: "standard",
          health_snapshot: {
            delivery: {
              state: "submitted",
              last_delivery_at: "2026-08-07T10:20:30Z",
              last_submission_evidence: "user_message_count_increased",
            },
          },
        },
      },
    });

    await act(async () => {
      poll?.();
      await Promise.resolve();
      await Promise.resolve();
    });
    await vi.waitFor(() => expect(apiMock.fetchSession).toHaveBeenCalledTimes(2));

    expect(apiMock.fetchSession).toHaveBeenLastCalledWith("group-1", "web-1", { inspect: false });
    expect(host.querySelector('[title="Submitted: user_message_count_increased"]')).not.toBeNull();
  });

  it("defines the model-switching caveat and both modes in every locale", () => {
    for (const locale of [enChat, zhChat, jaChat]) {
      const delivery = locale.webModelDelivery;
      expect(delivery.modeTitle).toBeTruthy();
      expect(delivery.modeDescription).toBeTruthy();
      expect(delivery.modeStandard).toBeTruthy();
      expect(delivery.modeImageCompat).toBe("GPT Pro");
      expect(delivery.modeImageCompatDescription).toBeTruthy();
      expect(delivery.modeHelp).toBeTruthy();
      expect(delivery.modeSaveFailed).toBeTruthy();
      expect(delivery.copyMcpUrl).toBeTruthy();
      expect(delivery.mcpStartFirstHint).toBeTruthy();
      expect(delivery.actorStoppedSurface).toBeTruthy();
    }
  });
});
