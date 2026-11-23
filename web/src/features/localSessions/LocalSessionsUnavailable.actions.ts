import { act } from "react";
import { expect, vi } from "vite-plus/test";
import enLayout from "../../i18n/locales/en/layout.json";
import type { DeviceSessionSource, LocalSessionSource } from "../../types";
import type { LocalSessionDirectory } from "../../services/api/localSessionControls";
import { sessionPickerActions } from "./LocalSessionsButton.actions";
import { verifySourceControls } from "./LocalSourceControl.actions";

/** Extends the existing unready-source case; its original assertions stay intact. */
export async function verifyUnavailableSource({
  open,
  source,
  publish,
  delayNextRead,
  publishDevice,
}: {
  open: () => Promise<void>;
  source: LocalSessionSource;
  publish: (directory: LocalSessionDirectory) => void;
  delayNextRead: () => (directory: LocalSessionDirectory) => void;
  publishDevice?: (sources: DeviceSessionSource[]) => void;
}) {
  vi.useFakeTimers();
  const translate = (key: string) =>
    key.split(".").reduce<unknown>((v, k) => (v as Record<string, unknown>)[k], enLayout) as string;
  const { click, button, row, checkbox, primary } = sessionPickerActions(translate);
  publish({ sources: [{ ...source, connected: false }] });
  await open();
  expect(row(source.title).textContent).toContain(enLayout.localSessions.unavailable);
  expect(checkbox(source.title).disabled).toBe(true);
  expect(document.body.textContent).not.toContain(enLayout.localSessions.empty);
  expect(primary().disabled).toBe(true);
  // The hint names no cause; it leads to the troubleshooting steps.
  const [setupDetails, troubleshooting] = Array.from(document.querySelectorAll("details")).filter(
    (details) => !details.parentElement?.closest("details"),
  );
  expect(setupDetails?.open).toBe(false);
  const scroll = vi.fn();
  troubleshooting!.scrollIntoView = scroll;
  await click(button(enLayout.localSessions.showTroubleshooting));
  expect(troubleshooting?.open).toBe(true);
  expect(scroll).toHaveBeenCalledOnce();
  await click(setupDetails!.querySelector("summary"));
  expect(setupDetails?.open).toBe(true);
  expect(scroll).toHaveBeenCalledOnce();
  await click(button(enLayout.localSessions.showSessions));
  const capabilityStatus = (missing: string) =>
    Array.from(document.querySelectorAll('[role="status"]')).find(
      (node) =>
        node.textContent === enLayout.sourceControls.upgrade.replace("{{capabilities}}", missing),
    );
  expect(capabilityStatus("source_control, source_diagnostics")?.textContent || "").toContain(
    "API/daemon",
  );
  expect(document.body.textContent).not.toContain(enLayout.sourceControls.requirements);
  expect(button(enLayout.sourceControls.remove).disabled).toBe(true);
  publish({
    sources: [{ ...source, connected: false }],
    capabilities: { source_control: true, source_diagnostics: false },
    source_states: {
      [source.source_id]: {
        control: {
          supported: true,
          management_revision: "0",
          access_blocked: false,
          pause_complete: true,
        },
        diagnostic: { state: "unknown", checked_at: null },
      },
    },
  });
  await act(async () => vi.advanceTimersByTimeAsync(3000));
  expect(capabilityStatus("source_diagnostics")?.textContent || "").toContain(
    "Reloading the original app’s MCP alone cannot add these API capabilities.",
  );
  expect(button(enLayout.sourceControls.recheck).disabled).toBe(true);
  expect(button(enLayout.sourceControls.remove).disabled).toBe(false);
  await verifySourceControls(source, publish, delayNextRead, publishDevice);
}
