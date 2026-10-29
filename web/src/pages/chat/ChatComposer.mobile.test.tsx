// @vitest-environment happy-dom
import { act, useRef, useState } from "react";
import { createRoot } from "react-dom/client";
import { expect, it, vi } from "vite-plus/test";
import { useComposerStore } from "../../stores/useComposerStore";
import { useGroupStore } from "../../stores";
import { ChatComposer } from "./ChatComposer";

const voice = vi.hoisted(() => vi.fn());
vi.mock("react-i18next", () => ({ useTranslation: () => ({ t: (key: string) => key }) }));
vi.mock("./LazyVoiceSecretaryComposerControl", () => ({
  LazyVoiceSecretaryComposerControl: () => <button onClick={voice}>voice</button>,
}));

function Harness() {
  const composerRef = useRef<HTMLTextAreaElement>(null);
  const fileInputRef = useRef<HTMLInputElement>(null);
  const [tokens, setTokens] = useState<string[]>([]);
  const noop = () => {};
  return (
    <ChatComposer
      isDark={false}
      isSmallScreen
      selectedGroupId="g"
      actors={[]}
      destGroupId="g"
      setDestGroupId={noop}
      composerGroupSettled
      busy=""
      replyTarget={null}
      onCancelReply={noop}
      quotedPresentationRef={null}
      onClearQuotedPresentationRef={noop}
      quotedVoiceDocumentRef={null}
      onQuoteVoiceDocumentRef={noop}
      onClearQuotedVoiceDocumentRef={noop}
      toTokens={tokens}
      onToggleRecipient={(token) =>
        setTokens((current) => (current.includes(token) ? [] : [token]))
      }
      onClearRecipients={() => setTokens([])}
      composerFiles={[]}
      onRemoveComposerFile={noop}
      appendComposerFiles={noop}
      fileInputRef={fileInputRef}
      composerRef={composerRef}
      setComposerText={useComposerStore.getState().setComposerText}
      messageMode="send"
      setMessageMode={noop}
      onSendMessage={noop}
      showMentionMenu={false}
      setShowMentionMenu={noop}
      mentionSuggestions={[]}
      mentionSelectedIndex={0}
      setMentionSelectedIndex={noop}
      setMentionFilter={noop}
      setMentionKind={noop}
      setMentionActorScope={noop}
      setMentionTargetGroupId={noop}
      composerGroupMentionTokens={[]}
      setComposerGroupMentionTokens={noop}
      composerAgentMentionTokens={[]}
      setComposerAgentMentionTokens={noop}
      slashCommands={[]}
    />
  );
}

it("keeps a compact control in place until its first click and preserves recipient selection", async () => {
  Object.assign(globalThis, { IS_REACT_ACT_ENVIRONMENT: true });
  useComposerStore.setState({ composerText: "" });
  const originalRefreshSettings = useGroupStore.getState().refreshSettings;
  useGroupStore.setState({ refreshSettings: async () => {} });
  voice.mockClear();
  const host = document.createElement("div");
  document.body.append(host);
  const root = createRoot(host);
  try {
    await act(async () => root.render(<Harness />));
    const footer = host.querySelector("footer")!;
    const compact = () => footer.getAttribute("data-composer-compact") === "true";
    const mic = host.querySelector<HTMLButtonElement>("[data-composer-voice] button")!;
    expect(compact()).toBe(true);
    await act(async () => mic.dispatchEvent(new Event("pointerdown", { bubbles: true })));
    expect(compact()).toBe(true);
    await act(async () => mic.click());
    expect(voice).toHaveBeenCalledOnce();
    expect(compact()).toBe(false);
    await act(async () => document.body.dispatchEvent(new Event("pointerdown", { bubbles: true })));
    expect(compact()).toBe(true);

    const textarea = host.querySelector("textarea")!;
    await act(async () => textarea.focus());
    const recipient = Array.from(host.querySelectorAll<HTMLButtonElement>("button")).find(
      (button) => button.textContent === "@foreman",
    )!;
    await act(async () => {
      recipient.dispatchEvent(new Event("pointerdown", { bubbles: true }));
      textarea.blur();
      recipient.click();
    });
    expect(recipient.getAttribute("aria-pressed")).toBe("true");
    expect(compact()).toBe(false);
    await act(async () => document.body.dispatchEvent(new Event("pointerdown", { bubbles: true })));
    expect(compact()).toBe(true);
    expect(host.querySelector(".compact-composer-recipients")?.textContent).toContain("@foreman");
  } finally {
    await act(async () => root.unmount());
    useGroupStore.setState({ refreshSettings: originalRefreshSettings });
    host.remove();
  }
});
