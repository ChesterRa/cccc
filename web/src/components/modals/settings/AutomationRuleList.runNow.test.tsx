// @vitest-environment happy-dom
import React, { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, expect, it, vi } from "vite-plus/test";
import type { AutomationRule } from "../../../types";

vi.mock("react-i18next", () => ({ useTranslation: () => ({ t: (key: string) => key }) }));

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

const notifyRule: AutomationRule = {
  id: "standup",
  enabled: false,
  scope: "group",
  to: ["@foreman"],
  trigger: { kind: "interval", every_seconds: 7200 },
  action: { kind: "notify", snippet_ref: "standup" },
};
const stateRule: AutomationRule = {
  id: "night",
  enabled: true,
  scope: "group",
  to: [],
  trigger: { kind: "at", at: "2026-11-08T00:00:00Z" },
  action: { kind: "group_state", state: "paused" },
};

async function render(
  props: Partial<React.ComponentProps<typeof AutomationRuleList>> & { rules: AutomationRule[] },
) {
  const { rules, ...rest } = props;
  await act(async () => {
    root.render(
      <AutomationRuleList
        isDark={false}
        visibleRules={rules}
        status={{}}
        rulesBusy={false}
        showCompletedRules
        completedRuleIds={[]}
        onToggleShowCompleted={vi.fn()}
        onClearCompleted={vi.fn()}
        onToggleRuleEnabled={vi.fn()}
        onEditRule={vi.fn()}
        onDeleteRule={vi.fn()}
        {...rest}
      />,
    );
  });
}

const runButtons = () =>
  Array.from(host.querySelectorAll<HTMLButtonElement>("button")).filter(
    (button) => button.textContent === "ruleList.runNow",
  );

it("offers Run now on notification rules only, including a disabled one", async () => {
  const onRunRule = vi.fn();
  await render({ rules: [notifyRule, stateRule], onRunRule, groupState: "active" });
  const buttons = runButtons();
  expect(buttons).toHaveLength(1);
  expect(buttons[0].disabled).toBe(false);
  expect(buttons[0].title).toBe("ruleList.runNowHint");
  await act(async () => buttons[0].click());
  expect(onRunRule).toHaveBeenCalledWith("standup");
});

it.each([
  ["paused", "automation.runPaused"],
  ["stopped", "automation.runStopped"],
])("blocks Run now while the Group is %s and says why", async (groupState, hint) => {
  await render({ rules: [notifyRule], onRunRule: vi.fn(), groupState });
  const [button] = runButtons();
  expect(button.disabled).toBe(true);
  expect(button.title).toBe(hint);
});

it("blocks Run now while another change is in flight", async () => {
  await render({ rules: [notifyRule], onRunRule: vi.fn(), groupState: "active", rulesBusy: true });
  expect(runButtons()[0].disabled).toBe(true);
});

it("shows the outcome of a run on that rule only", async () => {
  await render({
    rules: [notifyRule, { ...notifyRule, id: "other" }],
    onRunRule: vi.fn(),
    groupState: "active",
    runNotice: { ruleId: "other", text: "Sent to claude-1.", tone: "ok" },
  });
  const notices = host.querySelectorAll("[data-status-tone=ok]");
  expect(notices).toHaveLength(1);
  expect(notices[0].textContent).toBe("Sent to claude-1.");
  expect(notices[0].closest("div[class]")?.parentElement?.textContent).toContain("other");
});
