// @vitest-environment happy-dom
import { act, type ComponentProps } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, it, vi } from "vite-plus/test";
import type { AutomationRuleSet, AutomationSnippetCatalog } from "../../../types";
import type { AutomationRuleEditorModal } from "./AutomationRuleEditorModal";
import type { AutomationSnippetModal } from "./AutomationSnippetModal";
import type { AutomationRuleList } from "./AutomationRuleList";

const mocks = vi.hoisted(() => ({
  fetchAutomation: vi.fn(),
  updateAutomation: vi.fn(),
  resetAutomationBaseline: vi.fn(),
  runAutomationRule: vi.fn(),
  t: (key: string) => key,
}));

vi.mock("react-i18next", () => ({ useTranslation: () => ({ t: mocks.t }) }));

vi.mock("../../../services/api", () => ({
  fetchAutomation: (...args: unknown[]) => mocks.fetchAutomation(...args),
  updateAutomation: (...args: unknown[]) => mocks.updateAutomation(...args),
  resetAutomationBaseline: (...args: unknown[]) => mocks.resetAutomationBaseline(...args),
  runAutomationRule: (...args: unknown[]) => mocks.runAutomationRule(...args),
}));

vi.mock("./AutomationRuleEditorModal", () => ({
  AutomationRuleEditorModal: ({
    ruleDraft,
    errorMessage,
    saveBusy,
    onRuleChange,
    onSave,
  }: ComponentProps<typeof AutomationRuleEditorModal>) =>
    ruleDraft ? (
      <div data-editor="rule">
        <output>{ruleDraft.id}</output>
        <input
          aria-label="Rule draft"
          value={ruleDraft.id}
          onChange={(event) => onRuleChange({ ...ruleDraft, id: event.target.value })}
        />
        <output data-editor-error>{errorMessage}</output>
        <button disabled={saveBusy} onClick={() => void onSave()} data-save-rule>
          Save rule
        </button>
      </div>
    ) : null,
}));
vi.mock("./AutomationSnippetModal", () => ({
  AutomationSnippetModal: ({
    open,
    snippets,
    saveBusy,
    onUpdateSnippet,
    onSave,
  }: ComponentProps<typeof AutomationSnippetModal>) =>
    open ? (
      <div data-editor="snippet">
        <input
          aria-label="Snippet draft"
          value={snippets.note || ""}
          onChange={(event) => onUpdateSnippet("note", event.target.value)}
        />
        <button disabled={saveBusy} onClick={() => void onSave()} data-save-snippet>
          Save snippet
        </button>
      </div>
    ) : null,
}));
vi.mock("./AutomationRuleList", () => ({
  AutomationRuleList: ({
    visibleRules,
    groupState,
    rulesBusy,
    onEditRule,
    onRunRule,
    onToggleRuleEnabled,
    runNotice,
    status,
    completedRuleIds,
  }: ComponentProps<typeof AutomationRuleList>) => (
    <>
      <output
        data-rules-busy={String(rulesBusy)}
        data-completed-rules={completedRuleIds.join(",")}
        data-group-state={groupState}
      >
        {visibleRules.map((rule) => (
          <button key={rule.id} disabled={rulesBusy} onClick={() => onEditRule(rule.id)}>
            {rule.id}
          </button>
        ))}
      </output>
      {/* Run controls sit outside <output>, whose text other tests compare exactly. */}
      <div data-run-controls>
        {visibleRules.map((rule) => (
          <span key={rule.id} data-rule={rule.id} data-enabled={String(rule.enabled !== false)}>
            <button disabled={rulesBusy} onClick={() => onRunRule?.(rule.id)}>
              {`run:${rule.id}`}
            </button>
            <button onClick={() => onToggleRuleEnabled(rule.id, rule.enabled === false)}>
              {`toggle:${rule.id}`}
            </button>
            <span data-last-run>{status[rule.id]?.last_run_at || ""}</span>
          </span>
        ))}
        <span data-run-notice={runNotice?.ruleId || ""}>{runNotice?.text || ""}</span>
      </div>
    </>
  ),
}));

import { AutomationTab } from "./AutomationTab";

Object.assign(globalThis, { IS_REACT_ACT_ENVIRONMENT: true });

function deferred<T>() {
  let resolve!: (value: T) => void;
  let reject!: (error?: unknown) => void;
  const promise = new Promise<T>((res, rej) => {
    resolve = res;
    reject = rej;
  });
  return { promise, resolve, reject };
}

function makeAutomationResponse(ruleId: string, configPath: string, version = 1) {
  const ruleset: AutomationRuleSet = {
    rules: [
      {
        id: ruleId,
        enabled: true,
        scope: "group",
        to: ["@foreman"],
        trigger: { kind: "interval", every_seconds: 600 },
        action: { kind: "notify", message: "ping" },
      },
    ],
    snippets: { note: `Snippet for ${ruleId}` },
  };
  const snippet_catalog: AutomationSnippetCatalog = {
    built_in: {},
    built_in_overrides: {},
    custom: {},
  };
  return {
    ok: true as const,
    result: {
      ruleset,
      snippet_catalog,
      version,
      status: {},
      config_path: configPath,
      supported_vars: [],
    },
  };
}

describe("AutomationTab group ownership fencing", () => {
  let host: HTMLDivElement;
  let root: Root;

  const renderTab = async (groupId?: string, groupState?: string) => {
    await act(async () => {
      root.render(
        <AutomationTab isDark={false} groupId={groupId} groupState={groupState} devActors={[]} />,
      );
    });
  };

  const click = (label: string) =>
    act(async () => {
      const button = [...host.querySelectorAll<HTMLButtonElement>("button")].find(
        (element) => element.textContent === label,
      );
      expect(button).toBeDefined();
      expect(button!.disabled).toBe(false);
      button!.click();
    });

  const editDraft = (value: string) =>
    act(async () => {
      const input = host.querySelector<HTMLInputElement>("[data-editor] input")!;
      Object.getOwnPropertyDescriptor(HTMLInputElement.prototype, "value")!.set!.call(input, value);
      input.dispatchEvent(new Event("input", { bubbles: true }));
    });

  beforeEach(() => {
    vi.resetAllMocks();
    host = document.createElement("div");
    document.body.append(host);
    root = createRoot(host);
  });

  afterEach(async () => {
    await act(async () => root.unmount());
    host.remove();
    vi.unstubAllGlobals();
  });

  it("passes Group state to the list without reloading a draft", async () => {
    mocks.fetchAutomation.mockResolvedValue(makeAutomationResponse("reminder", "/path/a"));
    await renderTab("g_a", "paused");
    expect(host.querySelector("output")?.getAttribute("data-group-state")).toBe("paused");
    await click("reminder");
    await editDraft("draft-name");
    await renderTab("g_a", "idle");
    expect(host.querySelector("output")?.getAttribute("data-group-state")).toBe("idle");
    expect(host.querySelector<HTMLInputElement>("[data-editor] input")?.value).toBe("draft-name");
    expect(mocks.fetchAutomation).toHaveBeenCalledTimes(1);
  });

  it("classifies a completed repeating rule with the other completed rules", async () => {
    const response = makeAutomationResponse("limited", "/path/a");
    response.result.ruleset.rules[0].trigger = {
      kind: "interval",
      every_seconds: 600,
      max_runs: 2,
    };
    response.result.status = { limited: { completed: true, fired_count: 2 } };
    mocks.fetchAutomation.mockResolvedValue(response);

    await renderTab("g_a");
    expect(host.querySelector("output")?.textContent).toBe("");
    expect(host.querySelector("output")?.getAttribute("data-completed-rules")).toBe("limited");
  });

  it("clears previous Group rules immediately on switch and ignores late Group A responses", async () => {
    const readA = deferred<ReturnType<typeof makeAutomationResponse>>();
    const readB = deferred<ReturnType<typeof makeAutomationResponse>>();
    mocks.fetchAutomation.mockImplementation((groupId: string) =>
      groupId === "g_a" ? readA.promise : readB.promise,
    );

    await renderTab("g_a");
    expect(host.querySelector("output")?.getAttribute("data-rules-busy")).toBe("true");

    await renderTab("g_b");
    expect(host.querySelector("output")?.textContent).toBe("");
    expect(host.querySelector("output")?.getAttribute("data-rules-busy")).toBe("true");

    await act(async () => {
      readA.resolve(makeAutomationResponse("rule-from-a", "/path/a", 2));
      await readA.promise;
    });
    expect(host.querySelector("output")?.textContent).toBe("");
    expect(host.textContent).not.toContain("/path/a");
    expect(host.querySelector("output")?.getAttribute("data-rules-busy")).toBe("true");

    await act(async () => {
      readB.resolve(makeAutomationResponse("rule-from-b", "/path/b", 5));
      await readB.promise;
    });
    expect(host.querySelector("output")?.textContent).toBe("rule-from-b");
    expect(host.textContent).toContain("/path/b");
    expect(host.querySelector("output")?.getAttribute("data-rules-busy")).toBe("false");
  });

  it.each([
    ["rule", "g_b"],
    ["rule", "g_a"],
    ["snippet", "g_b"],
  ] as const)(
    "keeps the new %s draft in %s when an earlier save's refresh finishes",
    async (editor, destination) => {
      const refresh = deferred<ReturnType<typeof makeAutomationResponse>>();
      mocks.fetchAutomation
        .mockResolvedValueOnce(makeAutomationResponse("rule_a", "/path/a"))
        .mockReturnValueOnce(refresh.promise)
        .mockResolvedValue(makeAutomationResponse("rule_b", "/path/b", 3));
      mocks.updateAutomation.mockResolvedValue({ ok: true, result: {} });

      await renderTab("g_a");
      await click(editor === "rule" ? "rule_a" : "automation.snippets");
      await click(editor === "rule" ? "Save rule" : "Save snippet");
      expect(mocks.updateAutomation).toHaveBeenCalledTimes(1);
      expect(mocks.fetchAutomation).toHaveBeenCalledTimes(2);

      await renderTab("g_b");
      if (destination === "g_a") await renderTab("g_a");
      await click(editor === "rule" ? "rule_b" : "automation.snippets");
      await editDraft("unsaved_current_draft");
      expect(host.querySelector<HTMLInputElement>("[data-editor] input")?.value).toBe(
        "unsaved_current_draft",
      );

      await act(async () => {
        refresh.resolve(makeAutomationResponse("stale_rule_a", "/path/stale-a"));
        await refresh.promise;
      });
      expect(host.querySelector<HTMLInputElement>("[data-editor] input")?.value).toBe(
        "unsaved_current_draft",
      );
      expect(host.textContent).not.toContain("/path/stale-a");
      expect(host.querySelector("[data-rules-busy]")?.getAttribute("data-rules-busy")).toBe(
        "false",
      );
    },
  );

  it.each(["save", "reset"] as const)(
    "does not restore an old %s conflict after leaving and returning to the same Group",
    async (operation) => {
      vi.stubGlobal("confirm", () => true);
      const refresh = deferred<ReturnType<typeof makeAutomationResponse>>();
      mocks.fetchAutomation
        .mockResolvedValueOnce(makeAutomationResponse("rule_a", "/path/a"))
        .mockReturnValueOnce(refresh.promise)
        .mockResolvedValue(makeAutomationResponse("current_rule", "/path/current", 5));
      const conflict = { ok: false, error: { code: "version_conflict", message: "Old conflict" } };
      mocks.updateAutomation.mockResolvedValue(conflict);
      mocks.resetAutomationBaseline.mockResolvedValue(conflict);

      await renderTab("g_a");
      if (operation === "save") await click("rule_a");
      await click(operation === "save" ? "Save rule" : "automation.resetToDefaults");
      expect(mocks.fetchAutomation).toHaveBeenCalledTimes(2);
      await renderTab("g_b");
      await renderTab("g_a");
      await click("current_rule");

      await act(async () => {
        refresh.resolve(makeAutomationResponse("stale_rule", "/path/stale"));
        await refresh.promise;
      });
      expect(host.querySelector("[data-editor-error]")?.textContent).toBe("");
      expect(host.textContent).not.toContain("automation.versionConflict");
      expect(host.textContent).not.toContain("/path/stale");
    },
  );

  it("refreshes and closes the current editor after a successful save", async () => {
    mocks.fetchAutomation
      .mockResolvedValueOnce(makeAutomationResponse("rule_a", "/path/a"))
      .mockResolvedValueOnce(makeAutomationResponse("saved_rule", "/path/saved", 2));
    mocks.updateAutomation.mockResolvedValue({ ok: true, result: {} });
    await renderTab("g_a");
    await click("rule_a");
    await editDraft("saved_rule");
    await click("Save rule");
    expect(mocks.updateAutomation).toHaveBeenCalledWith(
      "g_a",
      expect.objectContaining({ rules: [expect.objectContaining({ id: "saved_rule" })] }),
      1,
    );
    expect(host.querySelector("[data-editor]")).toBeNull();
    expect(host.textContent).toContain("/path/saved");
    expect(host.querySelector("[data-rules-busy]")?.getAttribute("data-rules-busy")).toBe("false");
  });
});

describe("AutomationTab Run now", () => {
  let host: HTMLDivElement;
  let root: Root;

  const twoRules = (status: Record<string, unknown> = {}) => {
    const response = makeAutomationResponse("reminder", "/path/a", 4);
    response.result.ruleset.rules.push({ ...response.result.ruleset.rules[0], id: "other" });
    response.result.status = status;
    return response;
  };

  const click = (label: string) =>
    act(async () => {
      const button = [...host.querySelectorAll<HTMLButtonElement>("button")].find(
        (element) => element.textContent === label,
      );
      expect(button).toBeDefined();
      button!.click();
    });

  const notice = () => host.querySelector("[data-run-notice]")?.textContent || "";

  beforeEach(async () => {
    vi.resetAllMocks();
    host = document.createElement("div");
    document.body.append(host);
    root = createRoot(host);
    mocks.fetchAutomation.mockResolvedValue(twoRules());
    await act(async () => {
      root.render(
        <AutomationTab isDark={false} groupId="g_a" groupState="active" devActors={[]} />,
      );
    });
  });

  afterEach(async () => {
    await act(async () => root.unmount());
    host.remove();
  });

  it("runs the saved rule and refreshes only its status, keeping unsaved edits", async () => {
    await click("toggle:other");
    expect(host.querySelector("[data-rule=other]")?.getAttribute("data-enabled")).toBe("false");
    mocks.runAutomationRule.mockResolvedValue({
      ok: true,
      result: { rule_id: "reminder", run_at: "2026-11-07T12:30:00Z", notified: ["claude-1"] },
    });
    mocks.fetchAutomation.mockResolvedValue(
      twoRules({ reminder: { last_run_at: "2026-11-07T12:30:00Z", last_run_kind: "manual" } }),
    );

    await click("run:reminder");

    expect(mocks.runAutomationRule).toHaveBeenCalledWith("g_a", "reminder", 4);
    expect(notice()).toBe("automation.runSent");
    expect(host.querySelector("[data-rule=reminder] [data-last-run]")?.textContent).toBe(
      "2026-11-07T12:30:00Z",
    );
    // The unsaved toggle on the other rule survives the status refresh.
    expect(host.querySelector("[data-rule=other]")?.getAttribute("data-enabled")).toBe("false");
  });

  it("asks to save a rule with unsaved changes instead of running the saved one", async () => {
    await click("toggle:reminder");
    await click("run:reminder");
    expect(mocks.runAutomationRule).not.toHaveBeenCalled();
    expect(notice()).toBe("automation.runSaveFirst");
  });

  it.each([
    ["group_paused", "automation.runPaused"],
    ["group_stopped", "automation.runStopped"],
    ["no_notification", "automation.runNothingSent"],
    ["version_conflict", "automation.runConflict"],
    ["io_error", "automation.runUnknown"],
    ["daemon_unavailable", "automation.runUnknown"],
  ])("says what a %s refusal means without retrying", async (code, text) => {
    mocks.runAutomationRule.mockResolvedValue({ ok: false, error: { code, message: code } });
    await click("run:reminder");
    expect(notice()).toBe(text);
    expect(mocks.runAutomationRule).toHaveBeenCalledTimes(1);
  });

  it("treats a failed request as an unknown result", async () => {
    mocks.runAutomationRule.mockRejectedValue(new Error("network"));
    await click("run:reminder");
    expect(notice()).toBe("automation.runUnknown");
    expect(mocks.runAutomationRule).toHaveBeenCalledTimes(1);
  });
});
