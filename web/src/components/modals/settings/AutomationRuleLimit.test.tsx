// @vitest-environment happy-dom
import React, { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, expect, it, vi } from "vite-plus/test";
import type { AutomationRule, AutomationRuleStatus } from "../../../types";

vi.mock("react-i18next", () => ({
  useTranslation: () => ({
    t: (key: string, values?: { done?: number; max?: number }) =>
      key === "ruleList.runProgress" ? `${values?.done}/${values?.max}` : key,
  }),
}));
vi.mock("../../SelectCombobox", () => ({
  SelectCombobox: ({
    items,
    value,
    onChange,
    ariaLabel,
    disabled,
  }: {
    items: Array<{ value: string; label: string }>;
    value: string;
    onChange: (value: string) => void;
    ariaLabel: string;
    disabled?: boolean;
  }) => (
    <select
      aria-label={ariaLabel}
      value={value}
      disabled={disabled}
      onChange={(event) => onChange(event.target.value)}
    >
      {items.map((item) => (
        <option key={item.value} value={item.value}>
          {item.label}
        </option>
      ))}
    </select>
  ),
}));

import { AutomationRuleEditorModal } from "./AutomationRuleEditorModal";
import { AutomationRuleList } from "./AutomationRuleList";
import { AUTOMATION_TIME_FORMAT } from "./automationUtils";

Object.assign(globalThis, { IS_REACT_ACT_ENVIRONMENT: true });

let host: HTMLDivElement;
let root: Root;

beforeEach(() => {
  host = document.createElement("div");
  document.body.append(host);
  root = createRoot(host);
});

afterEach(async () => {
  await act(async () => root.unmount());
  host.remove();
  vi.unstubAllEnvs();
});

const intervalRule = (): AutomationRule => ({
  id: "reminder",
  enabled: true,
  scope: "group",
  to: ["@foreman"],
  trigger: { kind: "interval", every_seconds: 600, max_runs: 2 },
  action: { kind: "notify", message: "hello" },
});

async function renderEditor(
  rule: AutomationRule,
  status: AutomationRuleStatus,
  onChange: (rule: AutomationRule | null) => void,
) {
  await act(async () => {
    root.render(
      <AutomationRuleEditorModal
        isDark={false}
        ruleDraft={rule}
        ruleStatus={status}
        isNewRule={false}
        errorMessage=""
        saveBusy={false}
        snippetIds={[]}
        snippets={{}}
        actorTargetOptions={[]}
        oneShotMode="after"
        oneShotAfterMinutes={30}
        onRuleChange={onChange}
        onClose={vi.fn()}
        onSetRulesErr={vi.fn()}
        onSetOneShotMode={vi.fn()}
        onSetOneShotAfterMinutes={vi.fn()}
        onSave={vi.fn()}
      />,
    );
  });
}

async function renderList(rule: AutomationRule, status: AutomationRuleStatus, groupState?: string) {
  await act(async () => {
    root.render(
      <AutomationRuleList
        isDark={false}
        groupState={groupState}
        visibleRules={[rule]}
        status={{ [rule.id]: status }}
        rulesBusy={false}
        showCompletedRules
        completedRuleIds={status.completed ? [rule.id] : []}
        onToggleShowCompleted={vi.fn()}
        onClearCompleted={vi.fn()}
        onToggleRuleEnabled={vi.fn()}
        onEditRule={vi.fn()}
        onDeleteRule={vi.fn()}
      />,
    );
  });
}

it("shows local next and error times without presenting the timer baseline as a previous run", async () => {
  vi.stubEnv("TZ", "Asia/Tokyo");
  const rule = intervalRule();
  const next = "2026-11-07T12:02:30Z";
  const error = "2026-11-07T11:04:03Z";
  const baseline = "2026-11-07T10:02:30Z";
  const expectedNext = new Date(next).toLocaleString(undefined, {
    ...AUTOMATION_TIME_FORMAT,
    timeZone: "Asia/Tokyo",
  });
  const expectedError = new Date(error).toLocaleString(undefined, {
    ...AUTOMATION_TIME_FORMAT,
    timeZone: "Asia/Tokyo",
  });
  expect(expectedNext).not.toBe(
    new Date(next).toLocaleString(undefined, { ...AUTOMATION_TIME_FORMAT, timeZone: "UTC" }),
  );
  expect(expectedNext).not.toMatch(/\d:\d\d:\d\d/);
  const status = {
    last_fired_at: baseline,
    next_fire_at: next,
    last_error: "error",
    last_error_at: error,
  };

  await renderList(rule, status, "active");
  expect(host.textContent).toContain(expectedNext);
  expect(host.textContent).not.toContain(baseline);
  expect(host.textContent).not.toContain("ruleList.lastShort");

  await renderEditor(rule, status, vi.fn());
  expect(document.body.textContent).toContain(expectedNext);
  expect(document.body.textContent).toContain(expectedError);
  expect(document.body.textContent).not.toContain(baseline);
  expect(document.body.textContent).not.toContain("ruleEditor.last");
});

it.each([
  ["group_paused", "reminder", "layout:statusPaused"],
  ["group_stopped", "reminder", "layout:statusStopped"],
  ["idle_standup", "standup", "ruleList.idleStandup"],
] as const)(
  "shows why a rule will not fire (%s) instead of a next time",
  async (reason, id, label) => {
    const rule = { ...intervalRule(), id };
    const status: AutomationRuleStatus = { next_fire_at: null, suppressed_reason: reason };

    await renderList(rule, status);
    expect(host.textContent).toContain(`ruleList.nextShort: ${label}`);

    await renderEditor(rule, status, vi.fn());
    expect(document.body.textContent).toContain(label);
  },
);

it.each(["active", "idle", undefined])(
  "shows a standup's next time when the Web state is %s and the daemon gives no reason",
  async (groupState) => {
    const rule = { ...intervalRule(), id: "standup" };
    const next = "2026-11-07T12:02:30Z";
    const status = { next_fire_at: next };
    const formattedNext = new Date(next).toLocaleString(undefined, AUTOMATION_TIME_FORMAT);

    await renderList(rule, status, groupState);
    expect(host.textContent).toContain(formattedNext);
    expect(host.textContent).not.toContain("ruleList.idleStandup");

    await renderEditor(rule, status, vi.fn());
    expect(document.body.textContent).toContain(formattedNext);
  },
);

it("shows no next time for a rule without a plan", async () => {
  const rule = intervalRule();
  await renderList(rule, {});
  expect(host.textContent).toContain("ruleList.nextShort: —");
});

it("shows the last run only from a recorded run, marking a manual one", async () => {
  const rule = intervalRule();
  const ran = "2026-11-07T12:02:30Z";
  const formatted = new Date(ran).toLocaleString(undefined, AUTOMATION_TIME_FORMAT);

  // The clock base alone is not a run.
  await renderList(rule, { last_fired_at: ran, last_run_at: null });
  expect(host.textContent).not.toContain("ruleList.lastShort");

  await renderList(rule, { last_run_at: ran, last_run_kind: "scheduled" });
  expect(host.textContent).toContain(`ruleList.lastShort: ${formatted}`);
  expect(host.textContent).not.toContain("ruleList.lastRunManual");

  await renderList(rule, { last_run_at: ran, last_run_kind: "manual" });
  expect(host.textContent).toContain("ruleList.lastShort: ruleList.lastRunManual");

  await renderEditor(rule, { last_run_at: ran, last_run_kind: "scheduled" }, vi.fn());
  expect(document.body.textContent).toContain(`ruleEditor.last ${formatted}`);
});

it("does not show a fabricated completion time for an expired one-time rule", async () => {
  const rule = intervalRule();
  rule.trigger = { kind: "at", at: "2026-11-07T12:00:00Z" };
  const status = {
    completed: true,
    last_fired_at: "2026-11-07T13:00:00Z",
    completed_at: "2026-11-07T13:00:00Z",
  };

  await renderList(rule, status, "active");
  expect(host.textContent).toContain("ruleList.completedLabel");
  expect(host.textContent).not.toContain(status.completed_at);
  expect(host.textContent).not.toContain("ruleList.completedAt");

  await renderEditor(rule, status, vi.fn());
  expect(document.body.textContent).toContain("ruleList.completedLabel");
  expect(document.body.textContent).not.toContain(status.completed_at);
  expect(document.body.textContent).not.toContain("ruleEditor.completed");
});

async function changeInput(input: HTMLInputElement, value: string) {
  await act(async () => {
    Object.getOwnPropertyDescriptor(HTMLInputElement.prototype, "value")!.set!.call(input, value);
    input.dispatchEvent(new Event("input", { bubbles: true }));
  });
}

it("keeps the limit when the interval changes and removes it when cleared", async () => {
  const change = vi.fn();
  await renderEditor(intervalRule(), {}, change);
  const interval = [...document.querySelectorAll<HTMLInputElement>("input[type=number]")].find(
    (input) => input.id !== "automation-max-runs",
  );
  expect(interval).toBeDefined();
  await changeInput(interval!, "11");
  expect(change.mock.lastCall![0].trigger).toEqual({
    kind: "interval",
    every_seconds: 660,
    max_runs: 2,
  });

  await renderEditor(change.mock.lastCall![0], {}, change);
  await changeInput(document.querySelector<HTMLInputElement>("#automation-max-runs")!, "");
  expect(change.mock.lastCall![0].trigger).toEqual({ kind: "interval", every_seconds: 660 });
});

it("keeps the limit when the recurring time changes", async () => {
  const change = vi.fn();
  const rule = intervalRule();
  rule.trigger = { kind: "cron", cron: "0 9 * * *", timezone: "UTC", max_runs: 3 };
  await renderEditor(rule, {}, change);
  const time = document.querySelector<HTMLInputElement>("input[type=time]");
  expect(time).not.toBeNull();
  await changeInput(time!, "11:30");
  expect(change.mock.lastCall![0].trigger).toMatchObject({
    kind: "cron",
    cron: "30 11 * * *",
    max_runs: 3,
  });
});

it("keeps the limit between repeating schedules and drops it for a one-time rule", async () => {
  const change = vi.fn();
  await renderEditor(intervalRule(), {}, change);
  const schedule = () =>
    document.querySelector<HTMLSelectElement>('select[aria-label="ruleEditor.scheduleType"]')!;
  await act(async () => {
    schedule().value = "cron";
    schedule().dispatchEvent(new Event("change", { bubbles: true }));
  });
  expect(change.mock.lastCall![0].trigger).toMatchObject({ kind: "cron", max_runs: 2 });
  await renderEditor(change.mock.lastCall![0], {}, change);
  await act(async () => {
    schedule().value = "at";
    schedule().dispatchEvent(new Event("change", { bubbles: true }));
  });
  expect(change.mock.lastCall![0].trigger).toMatchObject({ kind: "at" });
  expect(change.mock.lastCall![0].trigger).not.toHaveProperty("max_runs");
});

it("allows enabling a completed repeating rule after raising its limit", async () => {
  const change = vi.fn();
  const rule = intervalRule();
  const status = { fired_count: 2, completed: true };
  await renderEditor(rule, status, change);
  expect(document.body.textContent).toContain("ruleList.completedLabel");
  expect(document.body.textContent).not.toContain("ruleEditor.completed");
  const enabled = () => document.querySelector<HTMLInputElement>('input[type="checkbox"]')!;
  expect(enabled().disabled).toBe(true);
  await changeInput(document.querySelector<HTMLInputElement>("#automation-max-runs")!, "3");
  await renderEditor(change.mock.lastCall![0], status, change);
  expect(enabled().disabled).toBe(false);
});

it("does not show the previous repeating completion on a one-time draft", async () => {
  const rule = intervalRule();
  rule.trigger = { kind: "at", at: "2030-01-01T00:00:00Z" };
  await renderEditor(
    rule,
    { fired_count: 2, completed: true, last_fired_at: "yesterday" },
    vi.fn(),
  );
  expect(document.body.textContent).not.toContain("ruleEditor.completed");
});

it("shows repeating progress and completion without inventing a completion time", async () => {
  await act(async () => {
    root.render(
      <AutomationRuleList
        isDark={false}
        visibleRules={[intervalRule()]}
        status={{ reminder: { fired_count: 2, completed: true, last_fired_at: "yesterday" } }}
        rulesBusy={false}
        showCompletedRules
        completedRuleIds={["reminder"]}
        onToggleShowCompleted={vi.fn()}
        onClearCompleted={vi.fn()}
        onToggleRuleEnabled={vi.fn()}
        onEditRule={vi.fn()}
        onDeleteRule={vi.fn()}
      />,
    );
  });
  expect(host.textContent).toContain("2/2");
  expect(host.textContent).toContain("ruleList.completedLabel");
  expect(host.textContent).not.toContain("ruleList.completedAt");
  const ruleToggle = [...host.querySelectorAll<HTMLInputElement>('input[type="checkbox"]')].at(-1);
  expect(ruleToggle?.disabled).toBe(true);
});

it("shows a recurring rule's own zone when it is not the browser's", async () => {
  vi.stubEnv("TZ", "Asia/Tokyo");
  const rule = intervalRule();
  rule.trigger = { kind: "cron", cron: "0 9 * * *", timezone: "America/New_York" };
  await renderList(rule, {});
  expect(host.textContent).toContain("ruleList.scheduleDailyAt (America/New_York)");

  // The daemon reads a rule without a saved zone as UTC.
  rule.trigger = { kind: "cron", cron: "0 9 * * *" };
  await renderList(rule, {});
  expect(host.textContent).toContain("ruleList.scheduleDailyAt (UTC)");

  rule.trigger = { kind: "cron", cron: "0 9 * * *", timezone: "Asia/Tokyo" };
  await renderList(rule, {});
  expect(host.textContent).not.toContain("(Asia/Tokyo)");
});

it("keeps a recurring rule's saved zone when its time is edited", async () => {
  vi.stubEnv("TZ", "Asia/Tokyo");
  const change = vi.fn();
  const rule = intervalRule();
  rule.trigger = { kind: "cron", cron: "0 9 * * *", timezone: "America/New_York" };
  await renderEditor(rule, {}, change);
  expect(document.body.textContent).toContain("ruleEditor.timeZone");
  await changeInput(document.querySelector<HTMLInputElement>("input[type=time]")!, "11:30");
  expect(change.mock.lastCall![0].trigger).toMatchObject({
    kind: "cron",
    cron: "30 11 * * *",
    timezone: "America/New_York",
  });
});

it("shows an expression the patterns cannot show as written and keeps it until replaced", async () => {
  vi.stubEnv("TZ", "UTC");
  const change = vi.fn();
  const rule = intervalRule();
  rule.trigger = { kind: "cron", cron: "*/15 * * * *", timezone: "UTC" };
  await renderList(rule, {});
  expect(host.textContent).toContain("*/15 * * * *");
  expect(host.textContent).not.toContain("ruleList.scheduleDailyAt");

  await renderEditor(rule, {}, change);
  expect(document.querySelector("input[type=time]")).toBeNull();
  const pattern = document.querySelector<HTMLSelectElement>(
    "select[aria-label='ruleEditor.pattern']",
  );
  expect(pattern?.value).toBe("custom");
  expect(change).not.toHaveBeenCalled();

  await act(async () => {
    pattern!.value = "weekly";
    pattern!.dispatchEvent(new Event("change", { bubbles: true }));
  });
  expect(change.mock.lastCall![0].trigger).toMatchObject({
    kind: "cron",
    cron: "0 9 * * 1",
    timezone: "UTC",
  });
});

it.each(["99 99 * * *", "0 9 * * 8", "0 9 0 * *", "0 9 32 * *"])(
  "shows the out-of-range expression %s as written instead of clamping it",
  async (cron) => {
    vi.stubEnv("TZ", "UTC");
    const rule = intervalRule();
    rule.trigger = { kind: "cron", cron, timezone: "UTC" };
    await renderList(rule, {});
    expect(host.textContent).toContain(cron);
    expect(host.textContent).not.toMatch(/ruleList\.schedule(Daily|Weekly|Monthly)At/);
  },
);
