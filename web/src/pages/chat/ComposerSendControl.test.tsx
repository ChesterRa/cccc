// @vitest-environment happy-dom
import { act, useState } from "react";
import { createRoot } from "react-dom/client";
import { expect, it, vi } from "vite-plus/test";
import { ComposerSendControl } from "./ComposerSendControl";
import type { ComposerMessageMode } from "../../stores/useComposerStore";
vi.mock("react-i18next", () => ({
  useTranslation: () => ({
    t: (key: string, args?: { mode?: string; shortcut?: string }) =>
      args?.mode || (args?.shortcut ? `${key}(${args.shortcut})` : key),
  }),
}));
it("switches the single send label without sending and preserves permission gates", async () => {
  Object.assign(globalThis, { IS_REACT_ACT_ENVIRONMENT: true });
  const host = document.createElement("div");
  document.body.append(host);
  const root = createRoot(host),
    send = vi.fn();
  function Harness({ ready = false, canSend = true, busy = false }) {
    const [mode, setMode] = useState<ComposerMessageMode>("send"),
      [open, setOpen] = useState(false);
    return (
      <ComposerSendControl
        mode={mode}
        options={[
          { key: "send", label: "Send", description: "Send now" },
          { key: "mail", label: "Mail", description: "Inbox only" },
          { key: "request_reply", label: "Send + Reply", description: "Require reply" },
        ]}
        open={open}
        onOpenChange={setOpen}
        onModeChange={setMode}
        sendKey="mod_enter"
        onSendKeyChange={() => {}}
        modEnterShortcutLabel="⌘+Enter"
        onSend={send}
        busy={busy}
        canSend={canSend}
        modeDisabled={busy}
        requestReplyReady={ready}
        title="Send (⌘+Enter)"
      />
    );
  }
  const toggle = () => host.querySelector<HTMLButtonElement>('button[aria-haspopup="dialog"]')!;
  const option = (text: string) =>
    Array.from(document.querySelectorAll<HTMLButtonElement>('[role="dialog"] button')).find((b) =>
      b.textContent?.startsWith(text),
    )!;
  try {
    await act(async () => root.render(<Harness />));
    expect(host.textContent).toBe("Send");
    expect(host.querySelector('button[title="Send (⌘+Enter)"]')).not.toBeNull();
    await act(async () => toggle().click());
    expect(option("Send + Reply").disabled).toBe(true);
    const reasonId = option("Send + Reply").getAttribute("aria-describedby")!;
    expect(document.getElementById(reasonId)?.textContent).toBe("modeSendReplyRequiresConcrete");
    expect(document.querySelector('[role="group"]')?.getAttribute("aria-describedby")).toBe(
      reasonId,
    );
    await act(async () => option("Mail").click());
    expect(document.querySelector('[role="dialog"]')).toBeNull();
    expect(host.textContent).toBe("Mail");
    expect(send).not.toHaveBeenCalled();
    await act(async () =>
      host.querySelector<HTMLButtonElement>('button[aria-label="Mail"]')!.click(),
    );
    expect(send).toHaveBeenCalledOnce();
    await act(async () => root.render(<Harness ready canSend={false} />));
    expect(host.querySelector<HTMLButtonElement>('button[aria-label="Mail"]')!.disabled).toBe(true);
    expect(toggle().disabled).toBe(false);
    await act(async () => toggle().click());
    expect(option("Send + Reply").disabled).toBe(false);
    expect(document.getElementById(reasonId)).toBeNull();
    await act(async () => option("Send + Reply").click());
    expect(host.textContent).toBe("Send + Reply");
    await act(async () => root.render(<Harness busy />));
    expect(Array.from(host.querySelectorAll("button")).every((b) => b.disabled)).toBe(true);
    // While sending, the key's name and tip say so at once.
    const primary = host.querySelector<HTMLButtonElement>("[data-composer-send-primary]")!;
    expect(primary.getAttribute("aria-label")).toBe("sending");
    expect(primary.getAttribute("title")).toBe("sending");
    await act(async () => root.render(<Harness />));
    expect(primary.getAttribute("title")).toBe("Send (⌘+Enter)");
  } finally {
    await act(async () => root.unmount());
    host.remove();
  }
});

it("offers a stop control only while an attachment upload can be stopped", async () => {
  Object.assign(globalThis, { IS_REACT_ACT_ENVIRONMENT: true });
  const host = document.createElement("div");
  document.body.append(host);
  const root = createRoot(host);
  const stop = vi.fn();
  const render = (onStopUploadWaiting?: () => void) => (
    <ComposerSendControl
      mode="send"
      options={[{ key: "send", label: "Send", description: "Send now" }]}
      open={false}
      onOpenChange={() => {}}
      onModeChange={() => {}}
      sendKey="mod_enter"
      onSendKeyChange={() => {}}
      modEnterShortcutLabel="⌘+Enter"
      onSend={() => {}}
      onStopUploadWaiting={onStopUploadWaiting}
      busy
      canSend={false}
      modeDisabled
      requestReplyReady
      title="Send"
    />
  );
  try {
    await act(async () => root.render(render()));
    expect(host.querySelector('button[aria-label="stopUploadWaiting"]')).toBeNull();
    await act(async () => root.render(render(stop)));
    const button = host.querySelector<HTMLButtonElement>('button[aria-label="stopUploadWaiting"]')!;
    expect(button.title).toBe("stopUploadWaiting");
    await act(async () => button.click());
    expect(stop).toHaveBeenCalledOnce();
  } finally {
    await act(async () => root.unmount());
    host.remove();
  }
});

it("offers the send key beside the modes, marks the current one and reports a change", async () => {
  Object.assign(globalThis, { IS_REACT_ACT_ENVIRONMENT: true });
  const host = document.createElement("div");
  document.body.append(host);
  const root = createRoot(host),
    send = vi.fn(),
    changeKey = vi.fn();
  try {
    await act(async () =>
      root.render(
        <ComposerSendControl
          mode="send"
          options={[{ key: "send", label: "Send", description: "Send now" }]}
          open
          onOpenChange={() => {}}
          onModeChange={() => {}}
          sendKey="enter"
          onSendKeyChange={changeKey}
          modEnterShortcutLabel="Ctrl+Enter"
          onSend={send}
          busy={false}
          canSend
          modeDisabled={false}
          requestReplyReady
          title="Send (Enter)"
        />,
      ),
    );
    const group = document.querySelector<HTMLElement>("[data-composer-send-key]")!;
    expect(group.getAttribute("role")).toBe("group");
    expect(group.getAttribute("aria-labelledby")).toBeTruthy();
    const keys = Array.from(group.querySelectorAll<HTMLButtonElement>("button"));
    expect(keys.map((b) => b.getAttribute("aria-pressed"))).toEqual(["true", "false"]);
    // The modifier label follows the platform.
    expect(keys[1].textContent).toContain("Ctrl+Enter");
    // Each key names its full meaning for assistive technology and the tooltip.
    expect(keys[0].getAttribute("aria-label")).toBe("sendKeyEnter");
    expect(keys[1].getAttribute("aria-label")).toBe("sendKeyModEnter(Ctrl+Enter)");
    // The group is named by its visible caption, and the hint follows the chosen key.
    const labelId = group.getAttribute("aria-labelledby")!;
    expect(document.getElementById(labelId)?.textContent).toBe("sendKeyOptions");
    expect(document.querySelector("[data-composer-send-key-hint]")?.textContent).toBe(
      "sendKeyEnterHint",
    );
    await act(async () => keys[1].click());
    expect(changeKey).toHaveBeenCalledWith("mod_enter");
    // Choosing a key never sends.
    expect(send).not.toHaveBeenCalled();
  } finally {
    await act(async () => root.unmount());
    host.remove();
  }
});
