// @vitest-environment happy-dom
import { act, useRef, useState } from "react";
import { createRoot } from "react-dom/client";
import { afterEach, beforeEach, expect, it, vi } from "vite-plus/test";
import { useComposerStore } from "../../stores/useComposerStore";
import { useGroupStore } from "../../stores";
import type { LedgerEvent } from "../../types";
import { ChatComposer } from "./ChatComposer";
import type { ComposerMentionSuggestion } from "./chatMentionSuggestions";

vi.mock("react-i18next", () => ({ useTranslation: () => ({ t: (key: string) => key }) }));
vi.mock("./LazyVoiceSecretaryComposerControl", () => ({
  LazyVoiceSecretaryComposerControl: () => null,
}));

const spies = {
  send: vi.fn(),
  cancelReply: vi.fn(),
  showMentionMenu: vi.fn(),
  mentionIndex: vi.fn(),
};
const peer: ComposerMentionSuggestion = { kind: "agent", value: "peer1", label: "peer1" };
const sent: LedgerEvent = {
  id: "e1",
  kind: "chat.message",
  by: "user",
  ts: "2026-10-29T00:00:00Z",
  data: { text: "an earlier message" },
} as LedgerEvent;

function Harness({ mentionMenu = false }: { mentionMenu?: boolean }) {
  const composerRef = useRef<HTMLTextAreaElement>(null);
  const fileInputRef = useRef<HTMLInputElement>(null);
  const [tokens, setTokens] = useState<string[]>([]);
  const noop = () => {};
  return (
    <ChatComposer
      isDark={false}
      isSmallScreen={false}
      selectedGroupId="g"
      actors={[]}
      recentMessages={[sent]}
      destGroupId="g"
      setDestGroupId={noop}
      composerGroupSettled
      busy=""
      replyTarget={null}
      onCancelReply={spies.cancelReply}
      quotedPresentationRef={null}
      onClearQuotedPresentationRef={noop}
      quotedVoiceDocumentRef={null}
      onQuoteVoiceDocumentRef={noop}
      onClearQuotedVoiceDocumentRef={noop}
      toTokens={tokens}
      onToggleRecipient={(token) => setTokens([token])}
      onClearRecipients={() => setTokens([])}
      composerFiles={[]}
      onRemoveComposerFile={noop}
      appendComposerFiles={noop}
      fileInputRef={fileInputRef}
      composerRef={composerRef}
      setComposerText={useComposerStore.getState().setComposerText}
      messageMode="send"
      setMessageMode={noop}
      onSendMessage={spies.send}
      showMentionMenu={mentionMenu}
      setShowMentionMenu={spies.showMentionMenu}
      mentionSuggestions={mentionMenu ? [peer] : []}
      mentionSelectedIndex={0}
      setMentionSelectedIndex={spies.mentionIndex}
      setMentionFilter={noop}
      setMentionKind={noop}
      setMentionActorScope={noop}
      setMentionTargetGroupId={noop}
      composerGroupMentionTokens={[]}
      setComposerGroupMentionTokens={noop}
      composerAgentMentionTokens={[]}
      setComposerAgentMentionTokens={noop}
      slashCommands={[
        { name: "plan", command: "/plan", capabilityId: "builtin", sourceType: "builtin_command" },
      ]}
    />
  );
}

// Chrome and Firefox mark keys inside a composition with isComposing; Safari
// can confirm a composition with key "Enter" and only keyCode 229 to show it.
const COMPOSING = [
  { label: "isComposing", isComposing: true, keyCode: 0 },
  { label: "keyCode 229", isComposing: false, keyCode: 229 },
];

let host: HTMLDivElement;
let root: ReturnType<typeof createRoot>;
const originalRefreshSettings = useGroupStore.getState().refreshSettings;

beforeEach(() => {
  Object.assign(globalThis, { IS_REACT_ACT_ENVIRONMENT: true });
  useGroupStore.setState({ refreshSettings: async () => {} });
  for (const spy of Object.values(spies)) spy.mockClear();
  host = document.createElement("div");
  document.body.append(host);
  root = createRoot(host);
});

afterEach(async () => {
  await act(async () => root.unmount());
  useGroupStore.setState({ refreshSettings: originalRefreshSettings });
  useComposerStore.setState({ composerText: "" });
  host.remove();
});

async function render(text: string, mentionMenu = false) {
  useComposerStore.setState({ composerText: text });
  await act(async () => root.render(<Harness mentionMenu={mentionMenu} />));
  return host.querySelector("textarea")!;
}

async function type(textarea: HTMLTextAreaElement, value: string) {
  const setValue = Object.getOwnPropertyDescriptor(HTMLTextAreaElement.prototype, "value")!.set!;
  await act(async () => {
    setValue.call(textarea, value);
    textarea.dispatchEvent(new Event("input", { bubbles: true }));
  });
}

async function press(
  textarea: HTMLTextAreaElement,
  key: string,
  { isComposing = false, keyCode = 0, ctrlKey = false, metaKey = false } = {},
) {
  const event = new KeyboardEvent("keydown", {
    key,
    isComposing,
    ctrlKey,
    metaKey,
    bubbles: true,
    cancelable: true,
  });
  Object.defineProperty(event, "keyCode", { value: keyCode });
  await act(async () => textarea.dispatchEvent(event));
  return event.defaultPrevented;
}

const composerText = () => useComposerStore.getState().composerText;

it.each(COMPOSING)(
  "leaves the slash menu's keys to the input method ($label)",
  async (composing) => {
    const textarea = await render("");
    await type(textarea, "/");
    expect(host.textContent).toContain("/plan");
    for (const key of ["ArrowDown", "Tab", "Enter", "Escape"]) {
      expect(await press(textarea, key, composing)).toBe(false);
    }
    expect(composerText()).toBe("/");
    expect(host.textContent).toContain("/plan");

    await press(textarea, "Enter");
    expect(composerText()).toBe("/plan ");
  },
);

it.each(COMPOSING)(
  "leaves the mention menu's keys to the input method ($label)",
  async (composing) => {
    const textarea = await render("@pe", true);
    for (const key of ["ArrowDown", "ArrowUp", "Tab", "Enter", "Escape"]) {
      expect(await press(textarea, key, composing)).toBe(false);
    }
    expect(spies.mentionIndex).not.toHaveBeenCalled();
    expect(spies.showMentionMenu).not.toHaveBeenCalled();
    expect(composerText()).toBe("@pe");

    await press(textarea, "Enter");
    expect(spies.showMentionMenu).toHaveBeenCalledWith(false);
    expect(composerText()).toBe("@peer1 ");
  },
);

it.each(COMPOSING)(
  "does not send or cancel a reply while composing ($label)",
  async (composing) => {
    const textarea = await render("hello");
    expect(await press(textarea, "Enter", { ...composing, ctrlKey: true })).toBe(false);
    expect(await press(textarea, "Enter", { ...composing, metaKey: true })).toBe(false);
    await press(textarea, "Escape", composing);
    expect(spies.send).not.toHaveBeenCalled();
    expect(spies.cancelReply).not.toHaveBeenCalled();

    await press(textarea, "Escape");
    expect(spies.cancelReply).toHaveBeenCalledOnce();
    await press(textarea, "Enter", { ctrlKey: true });
    expect(spies.send).toHaveBeenCalledOnce();
  },
);

it("sends only with Ctrl or Cmd held, never on a bare Enter", async () => {
  const textarea = await render("hello");
  expect(await press(textarea, "Enter")).toBe(false);
  expect(spies.send).not.toHaveBeenCalled();
  expect(await press(textarea, "Enter", { ctrlKey: true })).toBe(true);
  expect(await press(textarea, "Enter", { metaKey: true })).toBe(true);
  expect(spies.send).toHaveBeenCalledTimes(2);
});

it.each(COMPOSING)("does not recall history while composing ($label)", async (composing) => {
  const textarea = await render("");
  expect(await press(textarea, "ArrowUp", composing)).toBe(false);
  expect(composerText()).toBe("");

  expect(await press(textarea, "ArrowUp")).toBe(true);
  expect(composerText()).toBe("an earlier message");
});
