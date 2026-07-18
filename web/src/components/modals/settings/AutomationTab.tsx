// AutomationTab configures built-in automation loops and user-authored scheduled rules.
import React, { useEffect, useMemo, useState } from "react";
import { useTranslation } from "react-i18next";

import * as api from "../../../services/api";
import type {
  Actor,
  AutomationRule,
  AutomationRuleAction,
  AutomationRuleSet,
  AutomationRuleStatus,
  AutomationSnippetCatalog,
} from "../../../types";
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
import { cardClass, dangerButtonClass, primaryButtonClass, secondaryButtonClass } from "./types";

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
  onResetPolicies: () => void;
}

type PersistCopy = { failureMessage: string; versionConflictMessage: string };

function cloneRule(rule: AutomationRule): AutomationRule {
  return JSON.parse(JSON.stringify(rule)) as AutomationRule;
}

function createRuleDraft(seed?: Partial<AutomationRule>): AutomationRule {
  const id = String(seed?.id || nowId("rule")).trim();
  return {
    id,
    enabled: seed?.enabled ?? true,
    scope: seed?.scope ?? "group",
    owner_actor_id: seed?.owner_actor_id ?? null,
    to: seed?.to ?? ["@foreman"],
    trigger: seed?.trigger ?? { kind: "interval", every_seconds: 900 },
    action: seed?.action ?? defaultNotifyAction(),
  };
}

export function AutomationTab(props: AutomationTabProps) {
  const { isDark } = props;
  const { t } = useTranslation("settings");

  const [rulesBusy, setRulesBusy] = useState(false);
  const [rulesErr, setRulesErr] = useState("");
  const [ruleset, setRuleset] = useState<AutomationRuleSet | null>(null);
  const [snippetCatalog, setSnippetCatalog] = useState<AutomationSnippetCatalog>({
    built_in: {},
    built_in_overrides: {},
    custom: {},
  });
  const [rulesVersion, setRulesVersion] = useState<number | undefined>(undefined);
  const [status, setStatus] = useState<Record<string, AutomationRuleStatus>>({});
  const [configPath, setConfigPath] = useState("");
  const [supportedVars, setSupportedVars] = useState<string[]>([]);

  const [snippetManagerOpen, setSnippetManagerOpen] = useState(false);
  const [newSnippetId, setNewSnippetId] = useState("");
  const [templateErr, setTemplateErr] = useState("");
  const [snippetDrafts, setSnippetDrafts] = useState<Record<string, string>>({});

  const [editingRuleDraft, setEditingRuleDraft] = useState<AutomationRule | null>(null);
  const [editingRuleSourceId, setEditingRuleSourceId] = useState<string | null>(null);
  const [editingRuleIsNew, setEditingRuleIsNew] = useState(false);
  const [editingOneShotMode, setEditingOneShotMode] = useState<"after" | "exact">("after");
  const [editingOneShotAfterMinutes, setEditingOneShotAfterMinutes] = useState(30);

  const [showCompletedRules, setShowCompletedRules] = useState(false);

  const loadRules = async () => {
    if (!props.groupId) return;
    setRulesBusy(true);
    setRulesErr("");
    try {
      const resp = await api.fetchAutomation(props.groupId);
      if (!resp.ok) {
        setRulesErr(resp.error?.message || t("automation.failedToLoad"));
        return;
      }
      setRuleset(resp.result.ruleset);
      setSnippetCatalog(
        resp.result.snippet_catalog || { built_in: {}, built_in_overrides: {}, custom: {} },
      );
      setRulesVersion(typeof resp.result.version === "number" ? resp.result.version : undefined);
      setStatus(resp.result.status || {});
      setConfigPath(String(resp.result.config_path || ""));
      setSupportedVars(
        Array.isArray(resp.result.supported_vars) ? resp.result.supported_vars.map(String) : [],
      );
    } catch {
      setRulesErr(t("automation.failedToLoad"));
    } finally {
      setRulesBusy(false);
    }
  };

  useEffect(() => {
    if (props.groupId) void loadRules();
    // eslint-disable-next-line react-hooks/exhaustive-deps -- reload on group switch
  }, [props.groupId]);

  const draft: AutomationRuleSet = ruleset || { rules: [], snippets: {} };
  const builtinSnippetDefaults = useMemo(
    () => ({ ...(snippetCatalog.built_in || {}) }),
    [snippetCatalog.built_in],
  );
  const builtinOverrideIds = useMemo(
    () => new Set(Object.keys(snippetCatalog.built_in_overrides || {})),
    [snippetCatalog.built_in_overrides],
  );
  const snippetIds = useMemo(() => {
    const all = new Set<string>([
      ...Object.keys(builtinSnippetDefaults || {}),
      ...Object.keys(draft.snippets || {}),
    ]);
    const builtInIds = Array.from(all)
      .filter((id) => builtinSnippetDefaults[id] !== undefined)
      .sort();
    const customIds = Array.from(all)
      .filter((id) => builtinSnippetDefaults[id] === undefined)
      .sort();
    return [...builtInIds, ...customIds];
  }, [builtinSnippetDefaults, draft.snippets]);
  const snippetModalIds = useMemo(() => {
    const all = new Set<string>([
      ...Object.keys(builtinSnippetDefaults || {}),
      ...Object.keys(snippetDrafts || {}),
    ]);
    const builtInIds = Array.from(all)
      .filter((id) => builtinSnippetDefaults[id] !== undefined)
      .sort();
    const customIds = Array.from(all)
      .filter((id) => builtinSnippetDefaults[id] === undefined)
      .sort();
    return [...builtInIds, ...customIds];
  }, [builtinSnippetDefaults, snippetDrafts]);

  const actorTargetOptions = useMemo(() => {
    const out: Array<{ value: string; label: string }> = [
      { value: "@foreman", label: "@foreman" },
      { value: "@peers", label: "@peers" },
      { value: "@all", label: "@all" },
    ];
    for (const actor of props.devActors || []) {
      if (!actor || !actor.id || actor.id === "user") continue;
      out.push({ value: actor.id, label: actor.title ? `${actor.id} (${actor.title})` : actor.id });
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
    setDraft({
      ...draft,
      rules: draft.rules.map((rule) => (rule.id === ruleId ? { ...rule, ...patch } : rule)),
    });
  };

  const buildPersistedRuleset = (source: AutomationRuleSet): AutomationRuleSet => {
    const normalizedRules = source.rules.map((rule) => {
      if (!rule.action || rule.action.kind !== "notify") return rule;
      const notifyAction = rule.action as Extract<AutomationRuleAction, { kind: "notify" }>;
      const { kind: _kind, title: _unusedTitle, ...rest } = notifyAction;
      return { ...rule, action: { kind: "notify", ...rest } as AutomationRuleAction };
    });
    return { ...source, rules: normalizedRules };
  };

  const normalizeRuleForEditor = (rule: AutomationRule): AutomationRule => {
    const next = cloneRule(rule);
    const kind = actionKind(next.action);
    const triggerKind = String(next.trigger?.kind || "interval");
    if (kind !== "notify" && triggerKind !== "at") {
      next.trigger = { kind: "at", at: new Date(Date.now() + 30 * 60 * 1000).toISOString() };
    }
    return next;
  };

  const openRuleEditor = (
    rule: AutomationRule,
    options: { sourceId: string | null; isNew: boolean },
  ) => {
    const normalized = normalizeRuleForEditor(rule);
    setEditingRuleDraft(normalized);
    setEditingRuleSourceId(options.sourceId);
    setEditingRuleIsNew(options.isNew);
    setEditingOneShotMode(
      String(normalized.trigger?.kind || "interval") === "at" && !options.isNew ? "exact" : "after",
    );
    setEditingOneShotAfterMinutes(30);
    setRulesErr("");
  };

  const openNewRule = () => {
    openRuleEditor(createRuleDraft(), { sourceId: null, isNew: true });
  };

  const openExistingRule = (ruleId: string) => {
    const rule = draft.rules.find((item) => String(item.id || "").trim() === ruleId);
    if (!rule) return;
    openRuleEditor(rule, { sourceId: ruleId, isNew: false });
  };

  const closeRuleEditor = () => {
    setEditingRuleDraft(null);
    setEditingRuleSourceId(null);
    setEditingRuleIsNew(false);
    setEditingOneShotMode("after");
    setEditingOneShotAfterMinutes(30);
  };

  const setEditingOneShotModeValue = (mode: "after" | "exact") => {
    setEditingOneShotMode(mode);
    if (mode !== "after") return;
    setEditingRuleDraft((prev) =>
      prev
        ? {
            ...prev,
            trigger: {
              kind: "at",
              at: new Date(Date.now() + editingOneShotAfterMinutes * 60 * 1000).toISOString(),
            },
          }
        : prev,
    );
  };

  const setEditingOneShotAfterMinutesValue = (minutes: number) => {
    const nextMinutes = clampInt(minutes, 1, 7 * 24 * 60);
    setEditingOneShotAfterMinutes(nextMinutes);
    setEditingRuleDraft((prev) =>
      prev
        ? {
            ...prev,
            trigger: {
              kind: "at",
              at: new Date(Date.now() + nextMinutes * 60 * 1000).toISOString(),
            },
          }
        : prev,
    );
  };

  const openSnippetManager = () => {
    setTemplateErr("");
    setRulesErr("");
    setNewSnippetId("");
    setSnippetDrafts({ ...builtinSnippetDefaults, ...(draft.snippets || {}) });
    setSnippetManagerOpen(true);
  };

  const closeSnippetManager = () => {
    setTemplateErr("");
    setNewSnippetId("");
    setSnippetManagerOpen(false);
  };

  const addSnippet = () => {
    const id = newSnippetId.trim();
    if (!id) {
      setTemplateErr(t("automation.validationSnippetNameRequired"));
      return;
    }
    if (!isValidId(id)) {
      setTemplateErr(t("automation.snippetInvalid"));
      return;
    }
    if (builtinSnippetDefaults[id] !== undefined) {
      setTemplateErr(t("automation.builtInSnippetReserved", { id }));
      return;
    }
    if (snippetDrafts[id] !== undefined) {
      setTemplateErr(t("automation.snippetExists", { id }));
      return;
    }
    setTemplateErr("");
    setNewSnippetId("");
    setSnippetDrafts((prev) => ({ ...prev, [id]: "" }));
  };

  const updateSnippet = (id: string, content: string) => {
    setSnippetDrafts((prev) => ({ ...prev, [id]: content }));
  };

  const deleteSnippet = (id: string) => {
    if (builtinSnippetDefaults[id] !== undefined) {
      return;
    }
    const ok = window.confirm(t("automation.deleteSnippetConfirm", { id }));
    if (!ok) return;
    setSnippetDrafts((prev) => {
      const next = { ...prev };
      delete next[id];
      return next;
    });
  };

  const resetBuiltinSnippet = (id: string) => {
    const fallback = builtinSnippetDefaults[id];
    if (fallback === undefined) return;
    setSnippetDrafts((prev) => ({ ...prev, [id]: fallback }));
  };

  const validateRuleset = (candidate: AutomationRuleSet): string | null => {
    const seen = new Set<string>();
    for (const rule of candidate.rules) {
      const id = String(rule.id || "").trim();
      if (!id) return t("automation.validationRuleNameRequired");
      if (!isValidId(id)) return t("automation.validationRuleNameInvalid", { id });
      if (seen.has(id)) return t("automation.validationRuleNameDuplicate", { id });
      seen.add(id);

      const triggerKind = String(rule.trigger?.kind || "interval");
      if (triggerKind === "interval") {
        const every = Number(
          rule.trigger && "every_seconds" in rule.trigger ? rule.trigger.every_seconds : 0,
        );
        if (!Number.isFinite(every) || every < 1)
          return t("automation.validationIntervalMin", { id });
      } else if (triggerKind === "cron") {
        const cronExpr = String(
          rule.trigger && "cron" in rule.trigger ? rule.trigger.cron : "",
        ).trim();
        if (!cronExpr) return t("automation.validationScheduleRequired", { id });
      } else if (triggerKind === "at") {
        const atRaw = String(rule.trigger && "at" in rule.trigger ? rule.trigger.at : "").trim();
        if (!atRaw) return t("automation.validationOneTimeRequired", { id });
        const atMillis = Date.parse(atRaw);
        if (!Number.isFinite(atMillis)) return t("automation.validationDateTimeInvalid", { id });
      } else {
        return t("automation.validationTriggerUnsupported", { id, kind: triggerKind });
      }

      const scope = String(rule.scope || "group");
      if (scope !== "group" && scope !== "personal")
        return t("automation.validationScopeInvalid", { id });
      if (scope === "personal" && !String(rule.owner_actor_id || "").trim()) {
        return t("automation.validationOwnerRequired", { id });
      }

      const recipients = Array.isArray(rule.to)
        ? rule.to.map((item) => String(item || "").trim()).filter(Boolean)
        : [];
      const kind = actionKind(rule.action);
      if (kind === "notify") {
        if (recipients.length === 0) return t("automation.validationRecipientRequired", { id });
        const snippetRef = String(
          rule.action && "snippet_ref" in rule.action ? rule.action.snippet_ref || "" : "",
        ).trim();
        const message = String(
          rule.action && "message" in rule.action ? rule.action.message || "" : "",
        ).trim();
        if (snippetRef && candidate.snippets[snippetRef] === undefined) {
          return t("automation.validationSnippetMissing", { id, snippet: snippetRef });
        }
        if (!snippetRef && !message) return t("automation.validationMessageRequired", { id });
      } else if (kind === "group_state") {
        if (triggerKind !== "at") return t("automation.validationGroupStateOneTimeOnly", { id });
        const targetState = String(
          rule.action && "state" in rule.action ? rule.action.state || "" : "",
        ).trim();
        if (!["active", "idle", "paused", "stopped"].includes(targetState)) {
          return t("automation.validationGroupStateTargetRequired", { id });
        }
      } else if (kind === "actor_control") {
        if (triggerKind !== "at") return t("automation.validationActorControlOneTimeOnly", { id });
        const operation = String(
          rule.action && "operation" in rule.action ? rule.action.operation || "" : "",
        ).trim();
        if (!["start", "stop", "restart"].includes(operation)) {
          return t("automation.validationActorControlOperationRequired", { id });
        }
        const targets = Array.isArray(
          rule.action && "targets" in rule.action ? rule.action.targets : [],
        )
          ? (rule.action as { targets?: string[] }).targets
              ?.map((item) => String(item || "").trim())
              .filter(Boolean) || []
          : [];
        if (targets.length === 0)
          return t("automation.validationActorControlTargetRequired", { id });
      }
    }

    for (const key of Object.keys(candidate.snippets || {})) {
      const id = String(key || "").trim();
      if (!id) return t("automation.validationSnippetNameRequired");
      if (!isValidId(id)) return t("automation.validationRuleNameInvalid", { id });
    }
    return null;
  };

  const persistRuleset = async (
    nextDraft: AutomationRuleSet,
    copy: PersistCopy,
  ): Promise<boolean> => {
    if (!props.groupId) return false;
    const err = validateRuleset(nextDraft);
    if (err) {
      setRulesErr(err);
      return false;
    }

    setRulesBusy(true);
    setRulesErr("");
    try {
      const resp = await api.updateAutomation(
        props.groupId,
        buildPersistedRuleset(nextDraft),
        rulesVersion,
      );
      if (!resp.ok) {
        const code = String(resp.error?.code || "").trim();
        if (code === "version_conflict") {
          await loadRules();
          setRulesErr(copy.versionConflictMessage);
          return false;
        }
        setRulesErr(resp.error?.message || copy.failureMessage);
        return false;
      }
      await loadRules();
      return true;
    } catch {
      setRulesErr(copy.failureMessage);
      return false;
    } finally {
      setRulesBusy(false);
    }
  };

  const saveRules = async (): Promise<boolean> =>
    persistRuleset(draft, {
      failureMessage: t("automation.failedToSave"),
      versionConflictMessage: t("automation.versionConflict"),
    });

  const saveRuleEditor = async (): Promise<void> => {
    if (!editingRuleDraft) return;
    const nextRules = editingRuleSourceId
      ? draft.rules.map((rule) =>
          String(rule.id || "").trim() === editingRuleSourceId ? editingRuleDraft : rule,
        )
      : [...draft.rules, editingRuleDraft];
    const ok = await persistRuleset(
      { ...draft, rules: nextRules },
      {
        failureMessage: t("automation.failedToSave"),
        versionConflictMessage: t("automation.versionConflict"),
      },
    );
    if (ok) closeRuleEditor();
  };

  const saveSnippetManager = async (): Promise<void> => {
    const ok = await persistRuleset(
      { ...draft, snippets: { ...builtinSnippetDefaults, ...snippetDrafts } },
      {
        failureMessage: t("automation.failedToSave"),
        versionConflictMessage: t("automation.versionConflict"),
      },
    );
    if (ok) closeSnippetManager();
  };

  const removeRule = (ruleId: string) => {
    setDraft({ ...draft, rules: draft.rules.filter((rule) => rule.id !== ruleId) });
    if (editingRuleSourceId === ruleId) closeRuleEditor();
  };

  const clearCompletedRules = async () => {
    if (!props.groupId) return;
    if (completedOneTimeRuleIds.length <= 0) {
      setRulesErr(t("automation.noCompletedToClear"));
      return;
    }
    const ok = window.confirm(
      t("automation.clearCompletedConfirm", { count: completedOneTimeRuleIds.length }),
    );
    if (!ok) return;

    const removing = new Set(completedOneTimeRuleIds);
    const nextDraft: AutomationRuleSet = {
      ...draft,
      rules: draft.rules.filter((rule) => !removing.has(String(rule.id || "").trim())),
    };

    const persisted = await persistRuleset(nextDraft, {
      failureMessage: t("automation.failedToClear"),
      versionConflictMessage: t("automation.versionConflictShort"),
    });
    if (persisted && editingRuleSourceId && removing.has(editingRuleSourceId)) {
      closeRuleEditor();
    }
  };

  const resetToBaseline = async () => {
    if (!props.groupId) return;
    const ok = window.confirm(t("automation.resetConfirm"));
    if (!ok) return;
    setRulesBusy(true);
    setRulesErr("");
    try {
      const resp = await api.resetAutomationBaseline(props.groupId, rulesVersion);
      if (!resp.ok) {
        const code = String(resp.error?.code || "").trim();
        if (code === "version_conflict") {
          await loadRules();
          setRulesErr(t("automation.versionConflictShort"));
          return;
        }
        setRulesErr(resp.error?.message || t("automation.failedToReset"));
        return;
      }
      closeRuleEditor();
      closeSnippetManager();
      await loadRules();
    } catch {
      setRulesErr(t("automation.failedToReset"));
    } finally {
      setRulesBusy(false);
    }
  };

  if (!props.groupId) {
    return (
      <div className={cardClass()}>
        <div className="text-sm text-[var(--color-text-secondary)]">
          {t("automation.openFromGroup")}
        </div>
      </div>
    );
  }

  const editingRuleStatus =
    editingRuleSourceId && status[editingRuleSourceId] ? status[editingRuleSourceId] : {};

  return (
    <div className="space-y-6 animate-in fade-in slide-in-from-bottom-2 duration-300">
      <div>
        <h3 className="text-sm font-medium text-[var(--color-text-secondary)]">
          {t("automation.title")}
        </h3>
        <p className="text-xs mt-1 text-[var(--color-text-muted)]">
          {t("automation.description")}{" "}
          <span className="font-mono break-all">
            {configPath || t("automation.configPathFallback")}
          </span>
          .
        </p>
      </div>

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
        onResetPolicies={props.onResetPolicies}
      />

      <Section
        isDark={isDark}
        icon={SparkIcon}
        title={t("automation.rulesTitle")}
        description={t("automation.rulesDescription")}
      >
        {rulesErr ? (
          <div className="text-xs text-rose-600 dark:text-rose-300">{rulesErr}</div>
        ) : null}

        <div className="space-y-2">
          <div className="flex flex-col sm:flex-row sm:items-center sm:justify-between gap-2">
            <div className="grid grid-cols-2 gap-2 sm:flex sm:items-center">
              <button
                type="button"
                className={`${secondaryButtonClass()} w-full sm:w-auto whitespace-nowrap`}
                onClick={openNewRule}
                disabled={rulesBusy}
                title={t("automation.createRuleTitle")}
              >
                {t("automation.newRule")}
              </button>
              <button
                type="button"
                className={`${secondaryButtonClass()} w-full sm:w-auto whitespace-nowrap`}
                onClick={openSnippetManager}
                disabled={rulesBusy}
                title={t("automation.manageSnippetsTitle")}
              >
                {t("automation.snippets")}
              </button>
            </div>

            <div className="grid grid-cols-2 gap-2 sm:flex sm:items-center">
              <button
                type="button"
                className={`${dangerButtonClass()} w-full sm:w-auto whitespace-nowrap`}
                onClick={resetToBaseline}
                disabled={rulesBusy}
                title={t("automation.resetTitle")}
              >
                {t("automation.resetToDefaults")}
              </button>
              <button
                type="button"
                className={`${primaryButtonClass(rulesBusy)} w-full sm:w-auto whitespace-nowrap`}
                onClick={() => void saveRules()}
                disabled={rulesBusy}
                title={t("automation.saveTitle")}
              >
                {rulesBusy ? t("automation.saving") : t("common:save")}
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
          onToggleRuleEnabled={(ruleId, enabled) => updateRule(ruleId, { enabled })}
          onEditRule={openExistingRule}
          onDeleteRule={removeRule}
        />

        <div className="mt-2 text-[11px] text-[var(--color-text-muted)]">
          {t("automation.editHint")}
        </div>
      </Section>

      <AutomationRuleEditorModal
        isDark={isDark}
        ruleDraft={editingRuleDraft}
        ruleStatus={editingRuleStatus}
        isNewRule={editingRuleIsNew}
        errorMessage={rulesErr}
        saveBusy={rulesBusy}
        snippetIds={snippetIds}
        snippets={{ ...builtinSnippetDefaults, ...(draft.snippets || {}) }}
        actorTargetOptions={actorTargetOptions}
        oneShotMode={editingOneShotMode}
        oneShotAfterMinutes={editingOneShotAfterMinutes}
        onRuleChange={setEditingRuleDraft}
        onClose={closeRuleEditor}
        onSetRulesErr={setRulesErr}
        onSetOneShotMode={setEditingOneShotModeValue}
        onSetOneShotAfterMinutes={setEditingOneShotAfterMinutesValue}
        onSave={saveRuleEditor}
      />

      <AutomationSnippetModal
        open={snippetManagerOpen}
        isDark={isDark}
        templateErr={templateErr}
        saveErr={rulesErr}
        saveBusy={rulesBusy}
        newSnippetId={newSnippetId}
        supportedVars={supportedVars}
        snippetIds={snippetModalIds}
        snippets={snippetDrafts}
        builtinSnippetDefaults={builtinSnippetDefaults}
        builtinOverrideIds={Array.from(builtinOverrideIds)}
        onClose={closeSnippetManager}
        onNewSnippetIdChange={setNewSnippetId}
        onAddSnippet={addSnippet}
        onDeleteSnippet={deleteSnippet}
        onResetBuiltinSnippet={resetBuiltinSnippet}
        onUpdateSnippet={updateSnippet}
        onSave={saveSnippetManager}
      />
    </div>
  );
}
