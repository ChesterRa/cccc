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
  const write = vi.fn(async (_value: string) => {});
  Object.defineProperty(navigator, "clipboard", {
    configurable: true,
    value: { writeText: write },
  });
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
    await click(button(labels.create));
    expect(button(labels.preparing).disabled).toBe(true);
    expect(preparation.getAttribute("aria-busy")).toBe("true");
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
    write.mockRejectedValueOnce(new Error("clipboard denied"));
    await click(button(labels.copyTicket));
    expect(document.querySelector('[role="alert"]')?.textContent).toContain(enCommon.copyFailed);
    await click(button(labels.copyTicket));
    expect(write).toHaveBeenLastCalledWith(ticket);
    expect(document.querySelector('[role="alert"]')).toBeNull();
    expect(preparation.textContent).toContain(enCommon.copied);
    await act(async () => vi.advanceTimersByTimeAsync(1001));
    expect(preparation.textContent).toContain(labels.ticketExpired);
    expect(button(labels.copyTicket).disabled).toBe(true);
    const calls = write.mock.calls.length;
    await click(button(labels.copyTicket));
    expect(write.mock.calls).toHaveLength(calls);
    await click(button(labels.createAnother));
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
    expect(button(labels.copyTicket).disabled).toBe(false);
    expect(preparation.textContent).not.toContain(labels.ticketExpired);
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
    if (previousClipboard) Object.defineProperty(navigator, "clipboard", previousClipboard);
    else Reflect.deleteProperty(navigator, "clipboard");
  }
}
