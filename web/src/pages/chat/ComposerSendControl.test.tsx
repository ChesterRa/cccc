// @vitest-environment happy-dom
import { act, useState } from "react";
import { createRoot } from "react-dom/client";
import { expect, it, vi } from "vite-plus/test";
import { ComposerSendControl } from "./ComposerSendControl";
import type { ComposerMessageMode } from "../../stores/useComposerStore";
vi.mock("react-i18next", () => ({
  useTranslation: () => ({ t: (key: string, args?: { mode?: string }) => args?.mode || key }),
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
    await act(async () => option("Send + Reply").click());
    expect(host.textContent).toBe("Send + Reply");
    await act(async () => root.render(<Harness busy />));
    expect(Array.from(host.querySelectorAll("button")).every((b) => b.disabled)).toBe(true);
  } finally {
    await act(async () => root.unmount());
    host.remove();
  }
});
