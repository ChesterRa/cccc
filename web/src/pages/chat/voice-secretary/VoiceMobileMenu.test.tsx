// @vitest-environment happy-dom
import { act } from "react";
import { createRoot } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, it, vi } from "vite-plus/test";
import { VoiceMobileMenu } from "./VoiceMobileMenu";

vi.mock("react-i18next", () => ({ useTranslation: () => ({ t: (key: string) => key }) }));

describe("mobile voice options", () => {
  let host: HTMLDivElement;
  let root: ReturnType<typeof createRoot>;
  const onModeChange = vi.fn();
  const onLanguageChange = vi.fn();
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
  async function open(settingsLocked = false, assistantEnabled = true, languageSaving = false) {
    await act(async () => {
      root.render(
        <VoiceMobileMenu
          disabled={false}
          settingsLocked={settingsLocked}
          assistantEnabled={assistantEnabled}
          mode="prompt"
          modes={[
            { key: "prompt", label: "Prompt" },
            { key: "document", label: "Document" },
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
  it("changes mode through the menu and closes it", async () => {
    await open();
    await act(async () => option("Document").click());
    expect(onModeChange).toHaveBeenCalledWith("document");
    expect(host.querySelector("button")!.getAttribute("aria-expanded")).toBe("false");
  });
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
  });
  it("keeps direct dictation workspace accessible without assistant-only actions", async () => {
    await open(false, false);
    expect(option("Document")).toBeUndefined();
    expect(option("Optimize")).toBeUndefined();
    await act(async () => option("Workspace").click());
    expect(onWorkspace).toHaveBeenCalledOnce();
  });
});
