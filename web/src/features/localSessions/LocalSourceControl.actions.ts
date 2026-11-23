import { act } from "react";
import { expect, vi } from "vite-plus/test";
import enLayout from "../../i18n/locales/en/layout.json";
import enCommon from "../../i18n/locales/en/common.json";
import type { DeviceSessionSource, LocalSessionSource } from "../../types";
import type { LocalSessionDirectory } from "../../services/api/localSessionControls";
import { sessionPickerActions } from "./LocalSessionsButton.actions";
import { sourceControlFixture } from "./LocalSourceControl.fixtures";

export async function verifySourceControls(
  source: LocalSessionSource,
  publish: (directory: LocalSessionDirectory) => void,
  delayNextRead: () => (directory: LocalSessionDirectory) => void,
  publishDevice?: (sources: DeviceSessionSource[]) => void,
) {
  const labels = enLayout.sourceControls;
  const translate = (key: string) =>
    key.split(".").reduce<unknown>((v, k) => (v as Record<string, unknown>)[k], enLayout) as string;
  const { click, button, row, checkbox, primary } = sessionPickerActions(translate);
  const fixture = sourceControlFixture(source, publish, publishDevice);
  vi.stubGlobal("fetch", fixture.request);
  fixture.refresh();
  await act(async () => vi.advanceTimersByTimeAsync(3000));
  expect(document.body.textContent).toContain(labels.reasons.unknown);
  expect(fixture.request).not.toHaveBeenCalled();
  await click(button(labels.recheck));
  expect(document.body.textContent).toContain(labels.reasons.thread_not_loaded);
  expect(document.body.textContent).toContain(labels.checkCached);
  expect(button(labels.checkLater).disabled).toBe(true);
  await act(async () => vi.advanceTimersByTimeAsync(3000));
  expect(fixture.request).toHaveBeenCalledTimes(1);
  const removeButton = button(labels.remove);
  await click(removeButton);
  expect(document.querySelector('[role="dialog"] [role="alert"]')).toBeNull();
  expect(document.body.textContent).toContain(labels.removeConfirm);
  await click(button(enCommon.cancel));
  await act(async () => vi.advanceTimersByTimeAsync(0));
  expect(document.activeElement).toBe(removeButton);
  expect(fixture.request).toHaveBeenCalledTimes(1);
  fixture.ready();
  await act(async () => vi.advanceTimersByTimeAsync(3000));
  await click(checkbox(source.title));
  expect(primary().disabled).toBe(false);
  await click(button(labels.remove));
  await click(button(labels.confirmRemove));
  expect(button(labels.working).disabled).toBe(true);
  expect(primary().disabled).toBe(true);
  const pendingCalls = fixture.request.mock.calls.length;
  await click(button(labels.working));
  expect(fixture.request).toHaveBeenCalledTimes(pendingCalls);
  await act(async () => fixture.failPending());
  expect(document.body.textContent).toContain(labels.unconfirmed);
  expect(checkbox(source.title).checked).toBe(true);
  const staleDirectory = fixture.snapshot();
  const finishStaleRead = delayNextRead();
  await act(async () => vi.advanceTimersByTimeAsync(3000));
  fixture.setMode("partial-error");
  await click(button(labels.confirmRemove));
  await act(async () => finishStaleRead(staleDirectory));
  expect(document.body.textContent).toContain(labels.partial);
  expect(document.body.textContent).toContain(labels.unconfirmed);
  expect(
    Array.from(document.querySelectorAll("label")).some((x) =>
      x.textContent?.includes(source.title),
    ),
  ).toBe(false);
  expect(document.body.textContent).toContain("Selected sessions: 0 / 16");
  await click(button(enCommon.cancel));
  await act(async () => vi.advanceTimersByTimeAsync(0));
  expect(document.activeElement).toBe(
    document.querySelector('#local-sessions-title h2[tabindex="-1"]'),
  );
  const removed = Array.from(document.querySelectorAll("details")).find((x) =>
    x.querySelector("summary")?.textContent?.includes(labels.removedList),
  )!;
  await click(removed.querySelector("summary"));
  expect(button(labels.restore).disabled).toBe(true);
  fixture.setMode("complete");
  await click(button(labels.finishPause));
  const confirmation = Array.from(document.querySelectorAll('[role="dialog"]')).at(-1)!;
  await click(
    Array.from(confirmation.querySelectorAll("button")).find(
      (x) => x.textContent === labels.finishPause,
    )!,
  );
  expect(document.body.textContent).toContain(labels.removedNotice);
  await act(async () => vi.advanceTimersByTimeAsync(3000));
  expect(document.activeElement).toBe(
    document.querySelector('#local-sessions-title h2[tabindex="-1"]'),
  );
  expect(
    Array.from(document.querySelectorAll("label")).some((x) =>
      x.textContent?.includes(source.title),
    ),
  ).toBe(false);
  expect(button(labels.restore).disabled).toBe(false);
  fixture.setMode("restore-conflict");
  await click(button(labels.restore));
  await click(button(labels.confirmRestore));
  expect(document.body.textContent).toContain(labels.conflict);
  expect(document.body.textContent).toContain(labels.removedList);
  fixture.setMode("complete");
  await click(button(labels.confirmRestore));
  expect(document.body.textContent).toContain(labels.restoredNotice);
  await act(async () => vi.advanceTimersByTimeAsync(0));
  expect(document.activeElement).toBe(
    document.querySelector('#local-sessions-title h2[tabindex="-1"]'),
  );
  expect(checkbox(source.title).checked).toBe(false);
  const bodies = fixture.request.mock.calls.map(([, init]) => JSON.parse(String(init?.body)));
  expect(bodies.at(-1)).toEqual({
    source_id: source.source_id,
    action: "restore",
    expected_revision: "management-conflict",
  });
  fixture.addRemote();
  await act(async () => vi.advanceTimersByTimeAsync(3000));
  expect(row(fixture.remote.title).closest("section")?.textContent).toContain(
    fixture.remote.node_name,
  );
  const remoteRow = row(fixture.remote.title).parentElement!;
  expect(
    Array.from(remoteRow.querySelectorAll("button")).filter(
      (x) => x.textContent === labels.remove || x.textContent === labels.recheck,
    ),
  ).toHaveLength(0);
  expect(fixture.request.mock.calls.every(([url]) => !String(url).includes("session_nodes"))).toBe(
    true,
  );
}
