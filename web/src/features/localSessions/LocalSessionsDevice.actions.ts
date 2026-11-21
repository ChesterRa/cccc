import { act } from "react";
import { expect, vi } from "vite-plus/test";
import enLayout from "../../i18n/locales/en/layout.json";
import enCommon from "../../i18n/locales/en/common.json";
import type { LocalSessionSource } from "../../types";
import { sessionPickerActions } from "./LocalSessionsButton.actions";

/** Network and clipboard are boundaries; real dialog, hook, timer and controls run. */
export async function verifyEmptyDeviceFlow({
  open,
  showSources,
  source,
}: {
  open: () => Promise<void>;
  showSources: () => void;
  source: LocalSessionSource;
}) {
  vi.useFakeTimers();
  const labels = enLayout.sessionNodes;
  const t = (key: string) =>
    key.split(".").reduce<unknown>((v, k) => (v as Record<string, unknown>)[k], enLayout) as string;
  const { click, button, row } = sessionPickerActions(t);
  const ticket = "synthetic-ticket-not-for-a-real-device";
  let complete!: (response: Response) => void;
  const request = vi.fn(async (_url: unknown, init?: RequestInit) => {
    if (init?.method === "POST")
      return new Promise<Response>((resolve) => {
        complete = resolve;
      });
    return Response.json({ ok: true, result: { supported: true, devices: [] } });
  });
  vi.stubGlobal("fetch", request);
  const previousClipboard = Object.getOwnPropertyDescriptor(navigator, "clipboard");
  const previousExec = Object.getOwnPropertyDescriptor(document, "execCommand");
  const write = vi.fn(async (_value: string) => {});
  const posts = () => request.mock.calls.filter(([, init]) => init?.method === "POST").length;
  Object.defineProperty(navigator, "clipboard", {
    configurable: true,
    value: { writeText: write },
  });
  Object.defineProperty(document, "execCommand", { configurable: true, value: vi.fn(() => false) });
  try {
    await open();
    const statuses = Array.from(
      document.querySelectorAll('[role="status"]'),
      (el) => el.textContent,
    );
    expect(statuses).toContain(enLayout.localSessions.listening);
    const apps = document.querySelector(`ul[aria-label="${enLayout.localSessions.setupRuntime}"]`);
    expect(Array.from(apps?.querySelectorAll("li") || [], (li) => li.textContent)).toEqual([
      "Codex",
      "Claude Code",
      "Pi",
      "OpenCode",
    ]);
    for (const [title, description] of [
      [enLayout.localSessions.localSetup, enLayout.localSessions.localSetupDescription],
      [enLayout.localSessions.addDevice, enLayout.localSessions.deviceSetupDescription],
    ]) {
      const entry = button(title);
      expect(document.getElementById(entry.getAttribute("aria-describedby")!)?.textContent).toBe(
        description,
      );
    }
    await click(button(enLayout.localSessions.addDevice));
    const preparation = document.querySelector(`section[aria-label="${labels.prepareTitle}"]`)!;
    expect(preparation).not.toBeNull();
    expect(preparation.textContent).toContain(labels.runtimeHint);
    const command = "cccc sessions node connect --runtime codex --ticket-stdin --name laptop";
    expect(
      button(labels.copyConnectCommand).compareDocumentPosition(button(labels.create)) &
        Node.DOCUMENT_POSITION_FOLLOWING,
    ).not.toBe(0);
    await click(button(labels.copyConnectCommand));
    expect(write).toHaveBeenLastCalledWith(command);
    expect(write.mock.calls[0][0]).not.toContain(ticket);
    const legacy = Array.from(preparation.querySelectorAll("details")).find(
      (x) => x.querySelector("summary")?.textContent === labels.legacyTitle,
    )!;
    await click(legacy.querySelector("summary"));
    expect(legacy.open).toBe(true);
    await click(button(labels.copyLegacyCommands));
    expect(write).toHaveBeenLastCalledWith(
      "cccc sessions enable --runtime codex\ncccc sessions node connect --ticket-stdin --name laptop",
    );
    expect(legacy.textContent).toContain(labels.legacyHint);
    await click(button(labels.enableApp));
    await click(document.querySelector('[role="menuitemradio"][data-value="claude"]'));
    await click(button(labels.copyConnectCommand));
    expect(write).toHaveBeenLastCalledWith(
      "cccc sessions node connect --runtime claude --ticket-stdin --name laptop",
    );
    write.mockRejectedValueOnce(new Error("clipboard denied"));
    await click(button(labels.create));
    expect(button(labels.preparing).disabled).toBe(true);
    expect(preparation.getAttribute("aria-busy")).toBe("true");
    await click(button(labels.preparing));
    expect(posts()).toBe(1);
    await act(async () =>
      complete(
        Response.json({
          ok: true,
          result: { ticket, expires_at: new Date(Date.now() + 1000).toISOString() },
        }),
      ),
    );
    expect(preparation.textContent).toContain(labels.prepareHint);
    expect(document.body.textContent).not.toContain(labels.online);
    expect(write).toHaveBeenLastCalledWith(ticket);
    expect(document.querySelector('[role="alert"]')?.textContent).toContain(labels.copyFailed);
    expect(document.body.textContent).not.toContain(ticket);
    expect([...document.querySelectorAll("textarea")].some((area) => area.value === ticket)).toBe(
      false,
    );
    await click(button(labels.copyTicket));
    expect(write).toHaveBeenLastCalledWith(ticket);
    expect(posts()).toBe(1);
    expect(document.querySelector('[role="alert"]')).toBeNull();
    expect(preparation.textContent).toContain(enCommon.copied);
    write.mockRejectedValueOnce(new Error("clipboard denied"));
    await click(button(labels.createAnother));
    expect(posts()).toBe(2);
    const nextTicket = ticket + "-next-device";
    await act(async () =>
      complete(
        Response.json({
          ok: true,
          result: { ticket: nextTicket, expires_at: new Date(Date.now() + 1000).toISOString() },
        }),
      ),
    );
    expect(write).toHaveBeenLastCalledWith(nextTicket);
    await act(async () => vi.advanceTimersByTimeAsync(1001));
    expect(preparation.textContent).toContain(labels.ticketExpired);
    expect(button(labels.copyTicket)).toBeUndefined();
    expect(document.querySelector('[role="alert"]')?.textContent).not.toContain(labels.copyTicket);
    await click(button(labels.create));
    expect(posts()).toBe(3);
    await act(async () =>
      complete(
        Response.json({
          ok: true,
          result: {
            ticket: ticket + "-renewed",
            expires_at: new Date(Date.now() + 10000).toISOString(),
          },
        }),
      ),
    );
    expect(write).toHaveBeenLastCalledWith(ticket + "-renewed");
    expect(preparation.textContent).not.toContain(labels.ticketExpired);
    let finishCopy!: () => void;
    write.mockImplementationOnce(
      () =>
        new Promise<void>((resolve) => {
          finishCopy = resolve;
        }),
    );
    const copyButton = button(labels.copyTicket);
    const calls = write.mock.calls.length;
    await act(async () => {
      copyButton.click();
      copyButton.click();
    });
    expect(write.mock.calls).toHaveLength(calls + 1);
    expect(button(labels.copyingTicket).disabled).toBe(true);
    expect(button(labels.createAnother).disabled).toBe(true);
    await act(async () => finishCopy());
    expect(posts()).toBe(3);
    await click(
      Array.from(document.querySelectorAll("button")).find(
        (entry) =>
          entry.textContent === enLayout.localSessions.showSessions && !entry.closest("[hidden]"),
      )!,
    );
    expect(document.activeElement?.textContent).toBe(enLayout.localSessions.overviewTitle);
    showSources();
    await act(async () => vi.advanceTimersByTimeAsync(3000));
    expect(document.body.textContent).not.toContain(enLayout.localSessions.listening);
    expect(row(source.title).textContent).toContain(source.cwd);
  } finally {
    if (previousExec) Object.defineProperty(document, "execCommand", previousExec);
    else Reflect.deleteProperty(document, "execCommand");
    if (previousClipboard) Object.defineProperty(navigator, "clipboard", previousClipboard);
    else Reflect.deleteProperty(navigator, "clipboard");
  }
}
