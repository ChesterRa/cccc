// @vitest-environment happy-dom
import { act } from "react";
import { createRoot } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, it, vi } from "vite-plus/test";
import { VoiceMobileMenu } from "./VoiceMobileMenu";
import type { VoiceSecretaryCaptureMode } from "./voiceSecretaryTypes";

vi.mock("react-i18next", () => ({ useTranslation: () => ({ t: (key: string) => key }) }));

describe("mobile voice options", () => {
  let host: HTMLDivElement;
  let root: ReturnType<typeof createRoot>;
  const onModeChange = vi.fn();
  const onLanguageChange = vi.fn();
  const onPromptAutoRefineChange = vi.fn();
  const onOptimize = vi.fn();
  const onWorkspace = vi.fn();
  beforeEach(() => {
    (globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;
    vi.clearAllMocks();
    host = document.createElement("div");
    document.body.append(host);
    root = createRoot(host);
  });
  afterEach(async () => {
    await act(async () => root.unmount());
    host.remove();
  });
  async function open(
    settingsLocked = false,
    promptAutoRefine = false,
    languageSaving = false,
    disabled = false,
    mode: VoiceSecretaryCaptureMode = "prompt",
  ) {
    await act(async () => {
      root.render(
        <VoiceMobileMenu
          disabled={disabled}
          settingsLocked={settingsLocked}
          promptAutoRefine={promptAutoRefine}
          onPromptAutoRefineChange={onPromptAutoRefineChange}
          mode={mode}
          modes={[
            { key: "prompt", label: "Prompt" },
            { key: "document", label: "Document" },
            { key: "instruction", label: "Ask" },
          ]}
          onModeChange={onModeChange}
          language="mixed"
          languageDisabled={settingsLocked || languageSaving}
          languages={[
            { value: "mixed", label: "Mixed" },
            { value: "en-US", label: "English" },
          ]}
          onLanguageChange={onLanguageChange}
          optimizeLabel="Optimize"
          optimizeDisabled={false}
          onOptimize={onOptimize}
          workspaceLabel="Workspace"
          onWorkspace={onWorkspace}
        />,
      );
    });
    if (host.querySelector("button")!.getAttribute("aria-expanded") !== "true") {
      await act(async () => host.querySelector("button")!.click());
    }
  }
  function option(text: string) {
    return [...document.querySelectorAll<HTMLButtonElement>(".voice-mobile-menu button")].find(
      (b) => b.textContent === text,
    )!;
  }
  it.each([
    ["document", "Prompt", "prompt"],
    ["prompt", "Prompt ✓", "prompt"],
    ["prompt", "Document", "document"],
    ["prompt", "Ask", "instruction"],
  ] as const)(
    "closes the menu when selecting %s to %s",
    async (currentMode, label, selectedMode) => {
      await open(false, false, false, false, currentMode);
      await act(async () => option(label).click());
      expect(onModeChange).toHaveBeenCalledWith(selectedMode);
      expect(host.querySelector("button")!.getAttribute("aria-expanded")).toBe("false");
    },
  );
  it("changes language through the same menu", async () => {
    await open();
    await act(async () => option("English").click());
    expect(onLanguageChange).toHaveBeenCalledWith("en-US");
  });
  it("locks only language choices until the pending save settles", async () => {
    await open(false, true, true);
    expect(option("English").closest("fieldset")!.disabled).toBe(true);
    expect(option("Document").matches(":disabled")).toBe(false);
    expect(option("Workspace").matches(":disabled")).toBe(false);
    expect(onLanguageChange).not.toHaveBeenCalled();
    // Completion or failure both clear the parent's saving state.
    await open(false, true, false);
    expect(option("English").closest("fieldset")!.disabled).toBe(false);
    await act(async () => option("English").click());
    expect(onLanguageChange).toHaveBeenCalledOnce();
  });
  it("retains the recording lock on mode and language options", async () => {
    await open(true);
    expect(option("Document").closest("fieldset")!.disabled).toBe(true);
    expect(option("English").closest("fieldset")!.disabled).toBe(true);
    expect(option("Workspace").disabled).toBe(false);
    expect(document.querySelector<HTMLInputElement>('input[type="checkbox"]')!.disabled).toBe(true);
  });
  it("keeps Prompt polishing available without selecting Prompt or closing the menu", async () => {
    await open(false, false, false, false, "document");
    const toggle = document.querySelector<HTMLInputElement>('input[type="checkbox"]')!;
    expect(toggle.checked).toBe(false);
    await act(async () => toggle.click());
    expect(onPromptAutoRefineChange).toHaveBeenCalledWith(true);
    expect(onModeChange).not.toHaveBeenCalled();
    expect(onOptimize).not.toHaveBeenCalled();
    expect(host.querySelector("button")!.getAttribute("aria-expanded")).toBe("true");
    await open(false, true, false, false, "document");
    expect(document.querySelector<HTMLInputElement>('input[type="checkbox"]')!.checked).toBe(true);
    expect(option("Document ✓").getAttribute("aria-pressed")).toBe("true");
  });
  it("keeps mode and language choices accessible during dictation", async () => {
    await open(false, false);
    expect(option("Document")).toBeDefined();
    expect(option("English").closest("fieldset")!.disabled).toBe(false);
    expect(option("Optimize")).toBeDefined();
    await act(async () => option("Optimize").click());
    expect(onOptimize).toHaveBeenCalledOnce();
    await open(false, false);
    await act(async () => option("Workspace").click());
    expect(onWorkspace).toHaveBeenCalledOnce();
  });
  it("closes an open menu when its control becomes unavailable", async () => {
    await open();
    expect(option("Workspace")).toBeDefined();
    await open(false, true, false, true);
    expect(option("Workspace")).toBeUndefined();
    expect(host.querySelector("button")!.disabled).toBe(true);
    expect(onWorkspace).not.toHaveBeenCalled();
    expect(onModeChange).not.toHaveBeenCalled();
  });
});
