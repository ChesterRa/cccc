// @vitest-environment happy-dom
import { act } from "react";
import { createRoot } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, it, vi } from "vite-plus/test";
import { VoicePromptDraftReview } from "./VoicePromptDraftReview";
const copy = vi.hoisted(() => vi.fn(async () => true));
vi.mock("react-i18next", () => ({ useTranslation: () => ({ t: (key: string) => key }) }));
vi.mock("../../../utils/copy", () => ({ copyTextToClipboard: copy }));

describe("retained Prompt draft", () => {
  let host: HTMLDivElement;
  let root: ReturnType<typeof createRoot>;
  const onApply = vi.fn(async () => undefined);
  const onDismiss = vi.fn(async () => undefined);
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
  const button = (text: string) =>
    [...document.querySelectorAll<HTMLButtonElement>("button")].find(
      (button) => button.textContent === text,
    )!;
  const open = async (replace = true) => {
    await act(async () =>
      root.render(
        <VoicePromptDraftReview
          text="A retained result"
          replace={replace}
          onApply={onApply}
          onDismiss={onDismiss}
        />,
      ),
    );
    await act(async () => button("voiceSecretaryPromptDraftReview").click());
  };
  it("only adopts a draft after an explicit action; viewing and copying keep it pending", async () => {
    await open();
    expect(document.querySelector("textarea")?.value).toBe("A retained result");
    expect(onApply).not.toHaveBeenCalled();
    await act(async () => button("common:copy").click());
    expect(copy).toHaveBeenCalledWith("A retained result");
    expect(onApply).not.toHaveBeenCalled();
    expect(onDismiss).not.toHaveBeenCalled();
    await act(async () => button("voiceSecretaryPromptDraftReplace").click());
    expect(onApply).toHaveBeenCalledTimes(1);
  });
  it("lets the user discard an append candidate without applying it", async () => {
    await open(false);
    expect(button("voiceSecretaryPromptDraftAppend")).toBeDefined();
    await act(async () => button("voiceSecretaryPromptDraftDismiss").click());
    expect(onDismiss).toHaveBeenCalledTimes(1);
    expect(onApply).not.toHaveBeenCalled();
  });
});
