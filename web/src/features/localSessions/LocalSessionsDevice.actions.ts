import { act } from "react";
import { expect, vi } from "vite-plus/test";
import enLayout from "../../i18n/locales/en/layout.json";
import enCommon from "../../i18n/locales/en/common.json";
import { sessionPickerActions } from "./LocalSessionsButton.actions";

/** Network and clipboard are boundaries; real dialog, hook, timer and controls run. */
export async function verifyEmptyDeviceFlow(open: () => Promise<void>) {
  vi.useFakeTimers();
  const labels = enLayout.sessionNodes;
  const t = (key: string) =>
    key.split(".").reduce<unknown>((v, k) => (v as Record<string, unknown>)[k], enLayout) as string;
  const { click, button } = sessionPickerActions(t);
  const ticket = "synthetic-ticket-not-for-a-real-device";
  let complete!: (response: Response) => void;
  const request = vi.fn(async (_url: unknown, init?: RequestInit) => {
    if (init?.method === "POST")
      return new Promise<Response>((resolve) => {
        complete = resolve;
      });
    return Response.json({
      ok: true,
      result: { supported: true, devices: [], supported_runtimes: ["codex", "claude"] },
    });
  });
  vi.stubGlobal("fetch", request);
  const previousClipboard = Object.getOwnPropertyDescriptor(navigator, "clipboard");
  const previousExec = Object.getOwnPropertyDescriptor(document, "execCommand");
  const write = vi.fn(async (_value: string) => {});
  const written: string[] = [];
  class DeferredItem {
    private data: Record<string, Promise<Blob>>;
    constructor(data: Record<string, Promise<Blob>>) {
      this.data = data;
    }
    async getType(type: string) {
      return this.data[type];
    }
  }
  vi.stubGlobal("ClipboardItem", DeferredItem);
  const deferredWrite = vi.fn(async (items: ClipboardItem[]) => {
    const blob = await items[0].getType("text/plain");
    written.push(await blob.text());
  });
  const posts = () => request.mock.calls.filter(([, init]) => init?.method === "POST").length;
  Object.defineProperty(navigator, "clipboard", {
    configurable: true,
    value: { writeText: write, write: deferredWrite },
  });
  Object.defineProperty(document, "execCommand", { configurable: true, value: vi.fn(() => false) });
  const generate = async (label: string, writes: number) => {
    await act(async () => {
      button(label).click();
      expect(deferredWrite).toHaveBeenCalledTimes(writes);
    });
  };
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
    expect(document.querySelector("[data-modal-header] bdi")).toBeNull();
    expect(button(enLayout.localSessions.backToSessions)).toBeDefined();
    await click(button(enLayout.localSessions.backToSessions));
    expect(document.getElementById("local-sessions-title")?.textContent).toBe(
      enLayout.localSessions.overviewTitle,
    );
    await click(button(enLayout.localSessions.addDevice));
    const preparation = document.querySelector(`section[aria-label="${labels.prepareTitle}"]`)!;
    expect(preparation).not.toBeNull();
    expect(preparation.textContent).toContain(labels.scopeHint);
    expect(preparation.textContent).toContain(labels.runtimeHint);
    const commandPre = preparation.querySelector("pre")!;
    expect(commandPre.textContent).toBe(
      "cccc sessions node connect --runtime codex --ticket-stdin",
    );
    expect(commandPre.className).not.toContain("[overflow-wrap:anywhere]");
    expect(
      [...commandPre.querySelectorAll("span.whitespace-nowrap")].map((token) => token.textContent),
    ).toEqual(["cccc", "sessions", "node", "connect", "--runtime", "codex", "--ticket-stdin"]);
    const connectors = [
      ...preparation.querySelectorAll(`ol[aria-label="${labels.progress}"] li > span.border-t`),
    ];
    expect(connectors).toHaveLength(2);
    expect(
      connectors.every(
        (line) => line.classList.contains("hidden") && line.classList.contains("sm:inline-block"),
      ),
    ).toBe(true);
    const command = "cccc sessions node connect --runtime codex --ticket-stdin";
    expect(button(labels.create).disabled).toBe(false);
    expect(button(labels.copyConnectCommand).textContent).toBe(labels.copyCommandText);
    expect(posts()).toBe(0);
    expect(
      button(labels.copyConnectCommand).compareDocumentPosition(button(labels.create)) &
        Node.DOCUMENT_POSITION_FOLLOWING,
    ).not.toBe(0);
    await click(button(labels.copyConnectCommand));
    expect(write).toHaveBeenLastCalledWith(command);
    expect(write.mock.calls[0][0]).not.toContain(ticket);
    const codexNotes = Array.from(preparation.querySelectorAll("details")).find(
      (x) => x.querySelector("summary")?.textContent === labels.codexRequirements,
    )!;
    await click(codexNotes.querySelector("summary"));
    expect(codexNotes.open).toBe(true);
    expect(preparation.querySelectorAll("pre")).toHaveLength(1);
    expect(codexNotes.textContent).toContain(labels.codexRequirementHint);
    await click(button(labels.enableApp));
    await click(document.querySelector('[role="menuitemradio"][data-value="claude"]'));
    await click(button(labels.copyConnectCommand));
    expect(write).toHaveBeenLastCalledWith(
      "cccc sessions node connect --runtime claude --ticket-stdin",
    );
    expect(posts()).toBe(0);
    deferredWrite.mockRejectedValueOnce(new Error("clipboard denied immediately"));
    write.mockRejectedValueOnce(new Error("clipboard denied"));
    await generate(labels.create, 1);
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
    const alert = document.querySelector('[role="alert"]');
    expect(alert?.textContent).toContain(labels.copyFailed);
    expect(alert?.className).toContain("text-[var(--color-accent-danger)]");
    expect(document.body.textContent).not.toContain(ticket);
    expect([...document.querySelectorAll("textarea")].some((area) => area.value === ticket)).toBe(
      false,
    );
    await click(button(labels.showTicket));
    const revealed = document.querySelector<HTMLTextAreaElement>(
      `textarea[aria-label="${labels.ticketLabel}"]`,
    );
    expect(revealed?.readOnly).toBe(true);
    expect(revealed?.value).toBe(ticket);
    await click(button(labels.hideTicket));
    expect(document.querySelector(`textarea[aria-label="${labels.ticketLabel}"]`)).toBeNull();
    expect(document.body.textContent).not.toContain(ticket);
    await click(button(labels.showTicket));
    expect(
      document.querySelector<HTMLTextAreaElement>(`textarea[aria-label="${labels.ticketLabel}"]`)
        ?.value,
    ).toBe(ticket);
    await click(button(labels.copyTicket));
    expect(write).toHaveBeenLastCalledWith(ticket);
    expect(posts()).toBe(1);
    expect(document.querySelector('[role="alert"]')).toBeNull();
    expect(preparation.textContent).toContain(enCommon.copied);
    await generate(labels.createAnother, 2);
    expect(posts()).toBe(2);
    expect(written).toEqual([]);
    const nextTicket = ticket + "-next-device";
    await act(async () =>
      complete(
        Response.json({
          ok: true,
          result: { ticket: nextTicket, expires_at: new Date(Date.now() + 1000).toISOString() },
        }),
      ),
    );
    expect(written).toEqual([nextTicket]);
    expect(document.querySelector(`textarea[aria-label="${labels.ticketLabel}"]`)).toBeNull();
    await click(button(labels.showTicket));
    expect(
      document.querySelector<HTMLTextAreaElement>(`textarea[aria-label="${labels.ticketLabel}"]`)
        ?.value,
    ).toBe(nextTicket);
    await act(async () => vi.advanceTimersByTimeAsync(1001));
    expect(document.querySelector(`textarea[aria-label="${labels.ticketLabel}"]`)).toBeNull();
    expect(preparation.textContent).toContain(labels.ticketExpired);
    expect(button(labels.copyTicket)).toBeUndefined();
    expect(document.querySelector('[role="alert"]')).toBeNull();
    await generate(labels.create, 3);
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
    expect(written).toEqual([nextTicket, ticket + "-renewed"]);
    expect(preparation.textContent).not.toContain(labels.ticketExpired);
    let finishCopy!: () => void;
    write.mockImplementationOnce(
      () =>
        new Promise<void>((resolve) => {
          finishCopy = resolve;
        }),
    );
    const copyButton = [...preparation.querySelectorAll<HTMLButtonElement>("button")].find(
      (entry) => entry.title === labels.copyTicket,
    )!;
    expect(copyButton.textContent).toBe(enCommon.copied);
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
    vi.stubGlobal(
      "ClipboardItem",
      class {
        constructor() {
          throw new Error("Promise items unsupported");
        }
      },
    );
    await click(button(labels.createAnother));
    expect(posts()).toBe(4);
    const fallbackTicket = ticket + "-fallback";
    await act(async () =>
      complete(
        Response.json({
          ok: true,
          result: {
            ticket: fallbackTicket,
            expires_at: new Date(Date.now() + 10000).toISOString(),
          },
        }),
      ),
    );
    expect(write).toHaveBeenLastCalledWith(fallbackTicket);
    expect(posts()).toBe(4);
    vi.stubGlobal("ClipboardItem", DeferredItem);
    deferredWrite.mockRejectedValueOnce(new Error("clipboard denied before the hub responds"));
    await click(button(labels.createAnother));
    await act(async () =>
      complete(
        Response.json(
          { ok: false, error: { code: "forbidden", message: "Fixture access lost" } },
          { status: 403 },
        ),
      ),
    );
    expect(posts()).toBe(5);
    expect(document.body.textContent).toContain(labels.adminRequired);
    await act(async () => vi.advanceTimersByTimeAsync(5000));
    for (const [value, remaining, feedback] of [
      ["expired-fixture", -1, labels.ticketExpired],
      [" ", 10000, labels.ticketFailed],
    ] as const) {
      await click(button(labels.create));
      await act(async () =>
        complete(
          Response.json({
            ok: true,
            result: { ticket: value, expires_at: new Date(Date.now() + remaining).toISOString() },
          }),
        ),
      );
      expect(document.body.textContent).toContain(feedback);
    }
    expect(posts()).toBe(7);
    await click(button(labels.create));
    expect(posts()).toBe(8);
    await click(button(enCommon.close));
    await act(async () =>
      complete(
        Response.json({
          ok: true,
          result: {
            ticket: "cancelled-fixture",
            expires_at: new Date(Date.now() + 10000).toISOString(),
          },
        }),
      ),
    );
    expect(written).toEqual([nextTicket, ticket + "-renewed"]);
    expect(write).toHaveBeenLastCalledWith(fallbackTicket);
    expect(document.body.textContent).not.toContain("cancelled-fixture");
  } finally {
    if (previousExec) Object.defineProperty(document, "execCommand", previousExec);
    else Reflect.deleteProperty(document, "execCommand");
    if (previousClipboard) Object.defineProperty(navigator, "clipboard", previousClipboard);
    else Reflect.deleteProperty(navigator, "clipboard");
  }
}
