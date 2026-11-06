// @vitest-environment happy-dom
import { act, useRef, useState } from "react";
import { createRoot } from "react-dom/client";
import { expect, it, vi } from "vite-plus/test";
import { useComposerStore } from "../../stores/useComposerStore";
import { useGroupStore } from "../../stores";
import { useUIStore } from "../../stores/useUIStore";
import type { ReplyTarget } from "../../types";
import { ChatComposer } from "./ChatComposer";

const voice = vi.hoisted(() => vi.fn());
vi.mock("react-i18next", () => ({ useTranslation: () => ({ t: (key: string) => key }) }));
vi.mock("./LazyVoiceSecretaryComposerControl", () => ({
  LazyVoiceSecretaryComposerControl: () => <button onClick={voice}>voice</button>,
}));

const tapSettled = () => act(() => new Promise<void>((done) => setTimeout(done, 400)));
const press = (type: "pointerdown" | "pointerup") =>
  act(async () => document.body.dispatchEvent(new Event(type, { bubbles: true })));
async function tapOutside() {
  await press("pointerdown");
  await press("pointerup");
  await tapSettled();
}

function Harness({
  mode = "send",
  initialTokens = [],
  replyTarget = null,
  unavailableRecipients = [],
}: {
  mode?: "send" | "request_reply" | "mail";
  initialTokens?: string[];
  replyTarget?: ReplyTarget;
  unavailableRecipients?: string[];
}) {
  const composerRef = useRef<HTMLTextAreaElement>(null);
  const fileInputRef = useRef<HTMLInputElement>(null);
  const [tokens, setTokens] = useState<string[]>(initialTokens);
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
      sendInFlight={false}
      replyTarget={replyTarget}
      onCancelReply={noop}
      quotedPresentationRef={null}
      onClearQuotedPresentationRef={noop}
      quotedVoiceDocumentRef={null}
      onQuoteVoiceDocumentRef={noop}
      onClearQuotedVoiceDocumentRef={noop}
      toTokens={tokens}
      unavailableRecipients={unavailableRecipients}
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
      messageMode={mode}
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

it("uses the Web principal for Chet and shows Send instead of the saved Mail preference", async () => {
  Object.assign(globalThis, { IS_REACT_ACT_ENVIRONMENT: true });
  useComposerStore.setState({ composerText: "hello" });
  useUIStore.setState({ isWebAdmin: false });
  const originalRefreshSettings = useGroupStore.getState().refreshSettings;
  useGroupStore.setState({ refreshSettings: async () => {} });
  const host = document.createElement("div");
  document.body.append(host);
  const root = createRoot(host);
  const chet = () =>
    [...host.querySelectorAll<HTMLButtonElement>("button")].find(
      (button) => button.textContent === "@chet",
    );
  try {
    await act(async () => root.render(<Harness />));
    expect(chet()).toBeUndefined();
    await act(async () => useUIStore.setState({ isWebAdmin: true }));
    expect(chet()).toBeDefined();
    await act(async () =>
      root.render(<Harness key="selected" mode="mail" initialTokens={["@chet"]} />),
    );
    expect(host.querySelector("[data-composer-send-primary]")?.getAttribute("aria-label")).toBe(
      "sendMessage",
    );
    await act(async () =>
      host.querySelector<HTMLButtonElement>("[data-composer-send-options]")?.click(),
    );
    expect(document.querySelector('[role="dialog"]')?.textContent).not.toContain("modeMail");
  } finally {
    await act(async () => root.unmount());
    useUIStore.setState({ isWebAdmin: null });
    useGroupStore.setState({ refreshSettings: originalRefreshSettings });
    host.remove();
  }
});

it("keeps an unavailable recipient visible when the mobile composer is compact", async () => {
  Object.assign(globalThis, { IS_REACT_ACT_ENVIRONMENT: true });
  useComposerStore.setState({ composerText: "" });
  const originalRefreshSettings = useGroupStore.getState().refreshSettings;
  useGroupStore.setState({ refreshSettings: async () => {} });
  const host = document.createElement("div");
  document.body.append(host);
  const root = createRoot(host);
  try {
    await act(async () => root.render(<Harness unavailableRecipients={["departed"]} />));
    const compactChip = host.querySelector<HTMLButtonElement>(".compact-composer-recipients")!;
    expect(compactChip.textContent).toContain("unavailableRecipient");
    await act(async () => compactChip.click());
    expect(host.querySelector("[data-unavailable-recipient]")).not.toBeNull();
  } finally {
    await act(async () => root.unmount());
    useGroupStore.setState({ refreshSettings: originalRefreshSettings });
    host.remove();
  }
});

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
    await tapOutside();
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
    await tapSettled();
    expect(compact()).toBe(false);
    await tapOutside();
    expect(compact()).toBe(true);
    expect(host.querySelector(".compact-composer-recipients")?.textContent).toContain("@foreman");
  } finally {
    await act(async () => root.unmount());
    useGroupStore.setState({ refreshSettings: originalRefreshSettings });
    host.remove();
  }
});

it("stays put until a tap outside it has landed", async () => {
  Object.assign(globalThis, { IS_REACT_ACT_ENVIRONMENT: true });
  useComposerStore.setState({ composerText: "" });
  const originalRefreshSettings = useGroupStore.getState().refreshSettings;
  useGroupStore.setState({ refreshSettings: async () => {} });
  const host = document.createElement("div");
  document.body.append(host);
  const root = createRoot(host);
  try {
    await act(async () => root.render(<Harness />));
    const footer = host.querySelector("footer")!;
    const compact = () => footer.getAttribute("data-composer-compact") === "true";
    const textarea = host.querySelector("textarea")!;
    await act(async () => textarea.focus());
    expect(compact()).toBe(false);
    // The order a touch produces: pointerdown, then focus leaves, then the click.
    await act(async () => {
      document.body.dispatchEvent(new Event("pointerdown", { bubbles: true }));
      document.body.dispatchEvent(new Event("pointerup", { bubbles: true }));
      textarea.blur();
    });
    expect(compact()).toBe(false);
    await tapSettled();
    expect(compact()).toBe(true);
  } finally {
    await act(async () => root.unmount());
    useGroupStore.setState({ refreshSettings: originalRefreshSettings });
    host.remove();
  }
});

it("waits for a slow tap to lift before collapsing", async () => {
  Object.assign(globalThis, { IS_REACT_ACT_ENVIRONMENT: true });
  useComposerStore.setState({ composerText: "" });
  const originalRefreshSettings = useGroupStore.getState().refreshSettings;
  useGroupStore.setState({ refreshSettings: async () => {} });
  const host = document.createElement("div");
  document.body.append(host);
  const root = createRoot(host);
  try {
    await act(async () => root.render(<Harness />));
    const footer = host.querySelector("footer")!;
    const compact = () => footer.getAttribute("data-composer-compact") === "true";
    const mic = host.querySelector<HTMLButtonElement>("[data-composer-voice] button")!;
    await act(async () => mic.click());
    expect(compact()).toBe(false);

    await press("pointerdown");
    await tapSettled();
    expect(compact()).toBe(false);
    await press("pointerup");
    await tapSettled();
    expect(compact()).toBe(true);
  } finally {
    await act(async () => root.unmount());
    useGroupStore.setState({ refreshSettings: originalRefreshSettings });
    host.remove();
  }
});

it("stays open when focus returns before the tap settles", async () => {
  Object.assign(globalThis, { IS_REACT_ACT_ENVIRONMENT: true });
  useComposerStore.setState({ composerText: "" });
  const originalRefreshSettings = useGroupStore.getState().refreshSettings;
  useGroupStore.setState({ refreshSettings: async () => {} });
  const host = document.createElement("div");
  document.body.append(host);
  const root = createRoot(host);
  try {
    await act(async () => root.render(<Harness />));
    const footer = host.querySelector("footer")!;
    const textarea = host.querySelector("textarea")!;
    await act(async () => textarea.focus());
    await act(async () => {
      textarea.blur();
      textarea.focus();
    });
    await tapSettled();
    expect(footer.getAttribute("data-composer-compact")).toBeNull();
  } finally {
    await act(async () => root.unmount());
    useGroupStore.setState({ refreshSettings: originalRefreshSettings });
    host.remove();
  }
});

it("says under the recipients why Send + Reply cannot go to @all, and clears it when fixed", async () => {
  Object.assign(globalThis, { IS_REACT_ACT_ENVIRONMENT: true });
  useComposerStore.setState({ composerText: "check this" });
  const originalRefreshSettings = useGroupStore.getState().refreshSettings;
  useGroupStore.setState({ refreshSettings: async () => {} });
  const host = document.createElement("div");
  document.body.append(host);
  const root = createRoot(host);
  const reason = () => host.querySelector("[data-composer-send-blocked]")?.textContent ?? null;
  try {
    await act(async () => root.render(<Harness mode="send" initialTokens={["@all"]} />));
    expect(reason()).toBeNull();

    await act(async () =>
      root.render(<Harness key="reply" mode="request_reply" initialTokens={["@all"]} />),
    );
    expect(reason()).toBe("modeSendReplyRequiresConcrete");
    const all = Array.from(host.querySelectorAll<HTMLButtonElement>("button")).find(
      (button) => button.textContent === "@all",
    )!;
    await act(async () => all.click());
    expect(reason()).toBeNull();
  } finally {
    await act(async () => root.unmount());
    useComposerStore.setState({ composerText: "" });
    useGroupStore.setState({ refreshSettings: originalRefreshSettings });
    host.remove();
  }
});

it("answers Chet's message with Send when Mail is the saved preference", async () => {
  Object.assign(globalThis, { IS_REACT_ACT_ENVIRONMENT: true });
  useComposerStore.setState({ composerText: "hello" });
  useUIStore.setState({ isWebAdmin: true });
  const originalRefreshSettings = useGroupStore.getState().refreshSettings;
  useGroupStore.setState({ refreshSettings: async () => {} });
  const host = document.createElement("div");
  document.body.append(host);
  const root = createRoot(host);
  try {
    // A reply to Chet names no recipient; Chet has no Mail, so Mail is neither used nor offered.
    await act(async () =>
      root.render(
        <Harness
          mode="mail"
          replyTarget={{ eventId: "chet-event", by: "Chet", text: "Done?", toChet: true }}
        />,
      ),
    );
    expect(host.querySelector("[data-composer-send-primary]")?.getAttribute("aria-label")).toBe(
      "sendMessage",
    );
    await act(async () =>
      host.querySelector<HTMLButtonElement>("[data-composer-send-options]")?.click(),
    );
    expect(document.querySelector('[role="dialog"]')?.textContent).not.toContain("modeMail");
  } finally {
    await act(async () => root.unmount());
    useUIStore.setState({ isWebAdmin: null });
    useGroupStore.setState({ refreshSettings: originalRefreshSettings });
    host.remove();
  }
});
