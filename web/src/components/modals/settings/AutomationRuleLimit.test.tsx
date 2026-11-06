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
