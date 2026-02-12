// AutomationTab configures proactive system behaviors (nudges/alerts) and user-defined rules.
import React, { useEffect, useMemo, useState } from "react";

import * as api from "../../../services/api";
import type { Actor, AutomationRule, AutomationRuleAction, AutomationRuleSet, AutomationRuleStatus } from "../../../types";
import {
  Section,
  SparkIcon,
  actionKind,
  clampInt,
  defaultNotifyAction,
  isValidId,
  nowId,
} from "./automationUtils";
import { AutomationPoliciesSection } from "./AutomationPoliciesSection";
import { AutomationRuleEditorModal } from "./AutomationRuleEditorModal";
import { AutomationRuleList } from "./AutomationRuleList";
import { AutomationSnippetModal } from "./AutomationSnippetModal";
import { cardClass, primaryButtonClass } from "./types";

interface AutomationTabProps {
  isDark: boolean;
  groupId?: string;
  devActors: Actor[];
  busy: boolean;

  nudgeSeconds: number;
  setNudgeSeconds: (v: number) => void;
  replyRequiredNudgeSeconds: number;
  setReplyRequiredNudgeSeconds: (v: number) => void;
  attentionAckNudgeSeconds: number;
  setAttentionAckNudgeSeconds: (v: number) => void;
  unreadNudgeSeconds: number;
  setUnreadNudgeSeconds: (v: number) => void;
  nudgeDigestMinIntervalSeconds: number;
  setNudgeDigestMinIntervalSeconds: (v: number) => void;
  nudgeMaxRepeatsPerObligation: number;
  setNudgeMaxRepeatsPerObligation: (v: number) => void;
  nudgeEscalateAfterRepeats: number;
  setNudgeEscalateAfterRepeats: (v: number) => void;

  idleSeconds: number;
  setIdleSeconds: (v: number) => void;
  keepaliveSeconds: number;
  setKeepaliveSeconds: (v: number) => void;
  keepaliveMax: number;
  setKeepaliveMax: (v: number) => void;
  silenceSeconds: number;
  setSilenceSeconds: (v: number) => void;

  helpNudgeIntervalSeconds: number;
  setHelpNudgeIntervalSeconds: (v: number) => void;
  helpNudgeMinMessages: number;
  setHelpNudgeMinMessages: (v: number) => void;
  onSavePolicies: () => void;
}

export function AutomationTab(props: AutomationTabProps) {
  const { isDark } = props;

  const [rulesBusy, setRulesBusy] = useState(false);
  const [rulesErr, setRulesErr] = useState("");
  const [ruleset, setRuleset] = useState<AutomationRuleSet | null>(null);
  const [rulesVersion, setRulesVersion] = useState<number | undefined>(undefined);
  const [status, setStatus] = useState<Record<string, AutomationRuleStatus>>({});
  const [configPath, setConfigPath] = useState("");
  const [supportedVars, setSupportedVars] = useState<string[]>([]);
  const [newSnippetId, setNewSnippetId] = useState("");
  const [snippetManagerOpen, setSnippetManagerOpen] = useState(false);
  const [templateErr, setTemplateErr] = useState("");
  const [editingRuleId, setEditingRuleId] = useState<string | null>(null);
  const [oneShotModeByRule, setOneShotModeByRule] = useState<Record<string, "after" | "exact">>({});
  const [oneShotAfterMinutesByRule, setOneShotAfterMinutesByRule] = useState<Record<string, number>>({});
  const [showCompletedRules, setShowCompletedRules] = useState(false);

  const loadRules = async () => {
    if (!props.groupId) return;
    setRulesBusy(true);
    setRulesErr("");
    try {
      const resp = await api.fetchAutomation(props.groupId);
      if (!resp.ok) {
        setRulesErr(resp.error?.message || "Failed to load automation rules");
        return;
      }
      setRuleset(resp.result.ruleset);
      setRulesVersion(typeof resp.result.version === "number" ? resp.result.version : undefined);
      setStatus(resp.result.status || {});
      setConfigPath(String(resp.result.config_path || ""));
      setSupportedVars(Array.isArray(resp.result.supported_vars) ? resp.result.supported_vars.map(String) : []);
    } catch {
      setRulesErr("Failed to load automation rules");
    } finally {
      setRulesBusy(false);
    }
  };

  useEffect(() => {
    if (props.groupId) void loadRules();
    // eslint-disable-next-line react-hooks/exhaustive-deps -- reload on group switch
  }, [props.groupId]);

  const draft: AutomationRuleSet = ruleset || { rules: [], snippets: {} };
  const snippetIds = useMemo(() => Object.keys(draft.snippets || {}).sort(), [draft.snippets]);

  const actorTargetOptions = useMemo(() => {
    const out: Array<{ value: string; label: string }> = [
      { value: "@foreman", label: "@foreman" },
      { value: "@peers", label: "@peers" },
      { value: "@all", label: "@all" },
    ];
    for (const a of props.devActors || []) {
      if (!a || !a.id || a.id === "user") continue;
      out.push({ value: a.id, label: a.title ? `${a.id} (${a.title})` : a.id });
    }
    return out;
  }, [props.devActors]);

  const completedOneTimeRuleIds = useMemo(() => {
    return draft.rules
      .filter((rule) => {
        const rid = String(rule.id || "").trim();
        const triggerKind = String(rule.trigger?.kind || "");
        const st = status[rid] || {};
        return Boolean(rid) && triggerKind === "at" && Boolean(st.completed);
      })
      .map((rule) => String(rule.id || "").trim());
  }, [draft.rules, status]);

  const visibleRules = useMemo(() => {
    if (showCompletedRules) return draft.rules;
    return draft.rules.filter((rule) => {
      const rid = String(rule.id || "").trim();
      const triggerKind = String(rule.trigger?.kind || "");
      const st = status[rid] || {};
      return !(triggerKind === "at" && Boolean(st.completed));
    });
  }, [draft.rules, status, showCompletedRules]);

  const setDraft = (next: AutomationRuleSet) => setRuleset(next);

  const updateRule = (ruleId: string, patch: Partial<AutomationRule>) => {
    const next = { ...draft, rules: draft.rules.map((r) => (r.id === ruleId ? { ...r, ...patch } : r)) };
    setDraft(next);
  };

  const updateRuleNested = (ruleId: string, patch: Partial<AutomationRule>) => updateRule(ruleId, patch);

  const buildPersistedRuleset = (source: AutomationRuleSet): AutomationRuleSet => {
    const normalizedRules = source.rules.map((rule) => {
      if (!rule.action || rule.action.kind !== "notify") return rule;
      const notifyAction = rule.action as Extract<AutomationRuleAction, { kind: "notify" }>;
      const { kind: _kind, title: _unusedTitle, ...rest } = notifyAction;
      return { ...rule, action: { kind: "notify", ...rest } as AutomationRuleAction };
    });
    return { ...source, rules: normalizedRules };
  };

  const setOneShotAfterMinutes = (ruleId: string, minutes: number) => {
    const m = clampInt(minutes, 1, 7 * 24 * 60);
    setOneShotAfterMinutesByRule((prev) => ({ ...prev, [ruleId]: m }));
    updateRuleNested(ruleId, { trigger: { kind: "at", at: new Date(Date.now() + m * 60 * 1000).toISOString() } });
  };

  const addRule = (seed?: Partial<AutomationRule>) => {
    const id = String(seed?.id || nowId("rule")).trim();
    const nextRule: AutomationRule = {
      id,
      enabled: seed?.enabled ?? true,
      scope: seed?.scope ?? "group",
      owner_actor_id: seed?.owner_actor_id ?? null,
      to: seed?.to ?? ["@foreman"],
      trigger: seed?.trigger ?? { kind: "interval", every_seconds: 900 },
      action: seed?.action ?? defaultNotifyAction(),
    };
    setDraft({ ...draft, rules: [...draft.rules, nextRule] });
    return id;
  };

  const removeRule = (ruleId: string) => {
    setDraft({ ...draft, rules: draft.rules.filter((r) => r.id !== ruleId) });
    if (editingRuleId === ruleId) setEditingRuleId(null);
  };

  const addSnippet = () => {
    const id = newSnippetId.trim();
    if (!id) return;
    if (!isValidId(id)) {
      setTemplateErr("Snippet name is invalid. Use letters/numbers/_/- (max 64 chars).");
      return;
    }
    if (draft.snippets[id] !== undefined) {
      setTemplateErr(`Snippet already exists: ${id}`);
      return;
    }
    setTemplateErr("");
    setNewSnippetId("");
    setDraft({ ...draft, snippets: { ...draft.snippets, [id]: "" } });
  };

  const updateSnippet = (id: string, content: string) => {
    setDraft({ ...draft, snippets: { ...draft.snippets, [id]: content } });
  };

  const deleteSnippet = (id: string) => {
    const ok = window.confirm(`Delete snippet "${id}"?`);
    if (!ok) return;
    const next = { ...draft.snippets };
    delete next[id];
    setDraft({ ...draft, snippets: next });
  };

  const openSnippetManager = () => {
    setTemplateErr("");
    setSnippetManagerOpen(true);
  };

  const closeSnippetManager = () => {
    setTemplateErr("");
    setSnippetManagerOpen(false);
  };

  const validateBeforeSave = (): string | null => {
    const seen = new Set<string>();
    for (const r of draft.rules) {
      const id = String(r.id || "").trim();
      if (!id) return "Each rule needs a name (ID).";
      if (!isValidId(id)) return `Invalid rule name: ${id}`;
      if (seen.has(id)) return `Duplicate rule name: ${id}`;
      seen.add(id);
      const triggerKind = String(r.trigger?.kind || "interval");
      if (triggerKind === "interval") {
        const every = Number(r.trigger && "every_seconds" in r.trigger ? r.trigger.every_seconds : 0);
        if (!Number.isFinite(every) || every < 1) return `Rule "${id}": repeat interval must be at least 1 second.`;
      } else if (triggerKind === "cron") {
        const cronExpr = String(r.trigger && "cron" in r.trigger ? r.trigger.cron : "").trim();
        if (!cronExpr) return `Rule "${id}": schedule is required.`;
      } else if (triggerKind === "at") {
        const atRaw = String(r.trigger && "at" in r.trigger ? r.trigger.at : "").trim();
        if (!atRaw) return `Rule "${id}": one-time send time is required.`;
        const atMillis = Date.parse(atRaw);
        if (!Number.isFinite(atMillis)) return `Rule "${id}": invalid date/time format.`;
      } else {
        return `Rule "${id}": unsupported schedule type "${triggerKind}".`;
      }
      const scope = String(r.scope || "group");
      if (scope !== "group" && scope !== "personal") return `Rule "${id}": scope must be group or personal.`;
      if (scope === "personal" && !String(r.owner_actor_id || "").trim()) {
        return `Rule "${id}": personal rules require an owner.`;
      }
      const to = Array.isArray(r.to) ? r.to.map((x) => String(x || "").trim()).filter(Boolean) : [];
      const kind = actionKind(r.action);
      if (kind === "notify") {
        if (to.length === 0) return `Rule "${id}": please select at least one recipient.`;
        const snippetRef = String(r.action && "snippet_ref" in r.action ? r.action.snippet_ref || "" : "").trim();
        const msg = String(r.action && "message" in r.action ? r.action.message || "" : "").trim();
        if (snippetRef && draft.snippets[snippetRef] === undefined) {
          return `Rule "${id}": message snippet "${snippetRef}" does not exist.`;
        }
        if (!snippetRef && !msg) return `Rule "${id}": choose a message snippet or enter message text.`;
      } else if (kind === "group_state") {
        if (triggerKind !== "at") return `Rule "${id}": Set Group Status only supports One-Time schedule.`;
        const targetState = String(r.action && "state" in r.action ? r.action.state || "" : "").trim();
        if (!["active", "idle", "paused", "stopped"].includes(targetState)) {
          return `Rule "${id}": group state action requires active/idle/paused/stopped.`;
        }
      } else if (kind === "actor_control") {
        if (triggerKind !== "at") return `Rule "${id}": Control Actor Runtimes only supports One-Time schedule.`;
        const operation = String(r.action && "operation" in r.action ? r.action.operation || "" : "").trim();
        if (!["start", "stop", "restart"].includes(operation)) {
          return `Rule "${id}": actor control requires start/stop/restart.`;
        }
        const targets = Array.isArray(r.action && "targets" in r.action ? r.action.targets : [])
          ? (r.action as { targets?: string[] }).targets?.map((x) => String(x || "").trim()).filter(Boolean) || []
          : [];
        if (targets.length === 0) return `Rule "${id}": actor control requires at least one target.`;
      }
    }
    for (const k of Object.keys(draft.snippets || {})) {
      const id = String(k || "").trim();
      if (!id) return "Snippet name cannot be empty.";
      if (!isValidId(id)) return `Invalid snippet name: ${id}`;
    }
    return null;
  };

  const saveRules = async () => {
    if (!props.groupId) return;
    const err = validateBeforeSave();
    if (err) {
      setRulesErr(err);
      return;
    }
    setRulesBusy(true);
    setRulesErr("");
    try {
      const resp = await api.updateAutomation(props.groupId, buildPersistedRuleset(draft), rulesVersion);
      if (!resp.ok) {
        const code = String(resp.error?.code || "").trim();
          if (code === "version_conflict") {
            await loadRules();
            setRulesErr("Automation rules were updated elsewhere. Latest version loaded; please reapply your edits.");
            return;
          }
        setRulesErr(resp.error?.message || "Failed to save automation rules");
        return;
      }
      await loadRules();
    } catch {
      setRulesErr("Failed to save automation rules");
    } finally {
      setRulesBusy(false);
    }
  };

  const clearCompletedRules = async () => {
    if (!props.groupId) return;
    if (completedOneTimeRuleIds.length <= 0) {
      setRulesErr("No completed one-time reminders to clear.");
      return;
    }
    const ok = window.confirm(`Clear ${completedOneTimeRuleIds.length} completed one-time reminder(s)?`);
    if (!ok) return;
    const removing = new Set(completedOneTimeRuleIds);
    const nextRules = draft.rules.filter((rule) => !removing.has(String(rule.id || "").trim()));
    const nextDraft: AutomationRuleSet = { ...draft, rules: nextRules };
    setRulesBusy(true);
    setRulesErr("");
    try {
      const resp = await api.updateAutomation(props.groupId, buildPersistedRuleset(nextDraft), rulesVersion);
      if (!resp.ok) {
        const code = String(resp.error?.code || "").trim();
        if (code === "version_conflict") {
          await loadRules();
          setRulesErr("Automation rules were updated elsewhere. Latest version loaded.");
          return;
        }
        setRulesErr(resp.error?.message || "Failed to clear completed reminders");
        return;
      }
      if (editingRuleId && removing.has(editingRuleId)) {
        setEditingRuleId(null);
      }
      await loadRules();
    } catch {
      setRulesErr("Failed to clear completed reminders");
    } finally {
      setRulesBusy(false);
    }
  };

  const resetToBaseline = async () => {
    if (!props.groupId) return;
    const ok = window.confirm("Reset automation rules and message snippets to defaults? This replaces your current setup.");
    if (!ok) return;
    setRulesBusy(true);
    setRulesErr("");
    try {
      const resp = await api.resetAutomationBaseline(props.groupId, rulesVersion);
      if (!resp.ok) {
        const code = String(resp.error?.code || "").trim();
          if (code === "version_conflict") {
            await loadRules();
            setRulesErr("Automation rules were updated elsewhere. Latest version loaded.");
            return;
          }
        setRulesErr(resp.error?.message || "Failed to reset defaults");
        return;
      }
      await loadRules();
    } catch {
      setRulesErr("Failed to reset defaults");
    } finally {
      setRulesBusy(false);
    }
  };

  useEffect(() => {
    if (!editingRuleId) return;
    const rule = draft.rules.find((r) => String(r.id || "").trim() === editingRuleId);
    if (!rule) return;
    const kind = actionKind(rule.action);
    const triggerKind = String(rule.trigger?.kind || "interval");
    if (kind === "notify" || triggerKind === "at") return;
    const defaultAt = new Date(Date.now() + 30 * 60 * 1000).toISOString();
    setOneShotModeByRule((prev) => ({ ...prev, [editingRuleId]: "after" }));
    setOneShotAfterMinutesByRule((prev) => ({ ...prev, [editingRuleId]: prev[editingRuleId] ?? 30 }));
    updateRuleNested(editingRuleId, { trigger: { kind: "at", at: defaultAt } });
    // eslint-disable-next-line react-hooks/exhaustive-deps -- normalize edited rule only
  }, [editingRuleId, draft.rules]);

  if (!props.groupId) {
    return (
      <div className={cardClass(isDark)}>
        <div className={`text-sm ${isDark ? "text-slate-300" : "text-gray-700"}`}>Open this tab from a group.</div>
      </div>
    );
  }

  const editingRule = editingRuleId ? draft.rules.find((rule) => String(rule.id || "").trim() === editingRuleId) || null : null;
  const editingRuleStatus = editingRule ? status[String(editingRule.id || "").trim()] || {} : {};

  return (
    <div className="space-y-6 animate-in fade-in slide-in-from-bottom-2 duration-300">
      <div>
        <h3 className={`text-sm font-medium ${isDark ? "text-slate-300" : "text-gray-700"}`}>Automation</h3>
        <p className={`text-xs mt-1 ${isDark ? "text-slate-500" : "text-gray-500"}`}>
          Configure automation rules and policies. Settings are stored in{" "}
          <span className="font-mono break-all">{configPath || "CCCC_HOME/.../group.yaml"}</span>.
        </p>
      </div>

      <Section
        isDark={isDark}
        icon={SparkIcon}
        title="Automation Rules"
        description="Define trigger conditions and actions (notify, group state, actor control)."
      >
        {rulesErr ? <div className={`text-xs ${isDark ? "text-rose-300" : "text-rose-600"}`}>{rulesErr}</div> : null}

        <div className="space-y-2">
          <div className="flex flex-col sm:flex-row sm:items-center sm:justify-between gap-2">
            <div className="grid grid-cols-2 gap-2 sm:flex sm:items-center">
              <button
                type="button"
                className={`w-full px-3 py-2 rounded-lg text-sm min-h-[44px] font-medium transition-colors ${
                  isDark ? "bg-slate-800 hover:bg-slate-700 text-slate-200" : "bg-white hover:bg-gray-50 text-gray-800 border border-gray-200"
                } disabled:opacity-50`}
                onClick={() => {
                  const rid = addRule();
                  setEditingRuleId(rid);
                  setRulesErr("");
                }}
                disabled={rulesBusy}
                title="Create a rule"
              >
                <span className="whitespace-nowrap">New Rule</span>
              </button>
              <button
                type="button"
                className={`w-full px-3 py-2 rounded-lg text-sm min-h-[44px] font-medium transition-colors ${
                  isDark ? "bg-slate-800 hover:bg-slate-700 text-slate-200 border border-slate-700" : "bg-white hover:bg-gray-50 text-gray-800 border border-gray-200"
                } disabled:opacity-50`}
                onClick={openSnippetManager}
                disabled={rulesBusy}
                title="Manage snippets"
              >
                <span className="whitespace-nowrap">Snippets</span>
              </button>
            </div>

            <div className="grid grid-cols-2 gap-2 sm:flex sm:items-center">
              <button
                type="button"
                className={`w-full px-3 py-2 rounded-lg text-sm min-h-[44px] font-medium transition-colors ${
                  isDark ? "bg-rose-900/40 hover:bg-rose-900/60 text-rose-200 border border-rose-800" : "bg-rose-50 hover:bg-rose-100 text-rose-700 border border-rose-200"
                } disabled:opacity-50`}
                onClick={resetToBaseline}
                disabled={rulesBusy}
                title="Reset rules and snippets to defaults"
              >
                Reset to Defaults
              </button>
              <button
                type="button"
                className={`${primaryButtonClass(rulesBusy)} w-full sm:w-auto`}
                onClick={saveRules}
                disabled={rulesBusy}
                title="Save automation rules and snippets"
              >
                {rulesBusy ? "Saving..." : "Save"}
              </button>
            </div>
          </div>
        </div>

        <AutomationRuleList
          isDark={isDark}
          visibleRules={visibleRules}
          status={status}
          rulesBusy={rulesBusy}
          showCompletedRules={showCompletedRules}
          completedOneTimeRuleIds={completedOneTimeRuleIds}
          onToggleShowCompleted={setShowCompletedRules}
          onClearCompleted={clearCompletedRules}
          onToggleRuleEnabled={(ruleId, enabled) => updateRuleNested(ruleId, { enabled })}
          onEditRule={(ruleId) => {
            setEditingRuleId(ruleId);
            setRulesErr("");
          }}
          onDeleteRule={removeRule}
        />

        <div className={`mt-2 text-[11px] ${isDark ? "text-slate-500" : "text-gray-500"}`}>
          Edit opens full settings in a modal to keep this list clean. Message snippets are managed in Snippets.
        </div>
      </Section>

      <AutomationRuleEditorModal
        isDark={isDark}
        editingRule={editingRule}
        editingRuleStatus={editingRuleStatus}
        rulesErr={rulesErr}
        snippetIds={snippetIds}
        actorTargetOptions={actorTargetOptions}
        oneShotModeByRule={oneShotModeByRule}
        oneShotAfterMinutesByRule={oneShotAfterMinutesByRule}
        onRulePatch={updateRuleNested}
        onRuleRemove={removeRule}
        onSetEditingRuleId={setEditingRuleId}
        onSetRulesErr={setRulesErr}
        onSetOneShotMode={(ruleId, mode) => setOneShotModeByRule((prev) => ({ ...prev, [ruleId]: mode }))}
        onSetOneShotAfterMinutes={setOneShotAfterMinutes}
      />

      <AutomationPoliciesSection
        isDark={isDark}
        busy={props.busy}
        nudgeSeconds={props.nudgeSeconds}
        setNudgeSeconds={props.setNudgeSeconds}
        replyRequiredNudgeSeconds={props.replyRequiredNudgeSeconds}
        setReplyRequiredNudgeSeconds={props.setReplyRequiredNudgeSeconds}
        attentionAckNudgeSeconds={props.attentionAckNudgeSeconds}
        setAttentionAckNudgeSeconds={props.setAttentionAckNudgeSeconds}
        unreadNudgeSeconds={props.unreadNudgeSeconds}
        setUnreadNudgeSeconds={props.setUnreadNudgeSeconds}
        nudgeDigestMinIntervalSeconds={props.nudgeDigestMinIntervalSeconds}
        setNudgeDigestMinIntervalSeconds={props.setNudgeDigestMinIntervalSeconds}
        nudgeMaxRepeatsPerObligation={props.nudgeMaxRepeatsPerObligation}
        setNudgeMaxRepeatsPerObligation={props.setNudgeMaxRepeatsPerObligation}
        nudgeEscalateAfterRepeats={props.nudgeEscalateAfterRepeats}
        setNudgeEscalateAfterRepeats={props.setNudgeEscalateAfterRepeats}
        keepaliveSeconds={props.keepaliveSeconds}
        setKeepaliveSeconds={props.setKeepaliveSeconds}
        keepaliveMax={props.keepaliveMax}
        setKeepaliveMax={props.setKeepaliveMax}
        helpNudgeIntervalSeconds={props.helpNudgeIntervalSeconds}
        setHelpNudgeIntervalSeconds={props.setHelpNudgeIntervalSeconds}
        helpNudgeMinMessages={props.helpNudgeMinMessages}
        setHelpNudgeMinMessages={props.setHelpNudgeMinMessages}
        idleSeconds={props.idleSeconds}
        setIdleSeconds={props.setIdleSeconds}
        silenceSeconds={props.silenceSeconds}
        setSilenceSeconds={props.setSilenceSeconds}
        onSavePolicies={props.onSavePolicies}
      />

      <AutomationSnippetModal
        open={snippetManagerOpen}
        isDark={isDark}
        templateErr={templateErr}
        newSnippetId={newSnippetId}
        supportedVars={supportedVars}
        snippetIds={snippetIds}
        snippets={draft.snippets || {}}
        onClose={closeSnippetManager}
        onNewSnippetIdChange={setNewSnippetId}
        onAddSnippet={addSnippet}
        onDeleteSnippet={deleteSnippet}
        onUpdateSnippet={updateSnippet}
      />
    </div>
  );
}
