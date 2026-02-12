// AutomationTab configures proactive system behaviors (nudges/alerts) and user-defined rules.
import React, { useEffect, useMemo, useState } from "react";

import * as api from "../../../services/api";
import type { Actor, AutomationRule, AutomationRuleAction, AutomationRuleSet, AutomationRuleStatus } from "../../../types";
import { cardClass, inputClass, labelClass, primaryButtonClass } from "./types";

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

const BellIcon = ({ className }: { className?: string }) => (
  <svg
    xmlns="http://www.w3.org/2000/svg"
    width="16"
    height="16"
    viewBox="0 0 24 24"
    fill="none"
    stroke="currentColor"
    strokeWidth="2"
    strokeLinecap="round"
    strokeLinejoin="round"
    className={className}
  >
    <path d="M6 8a6 6 0 0 1 12 0c0 7 3 9 3 9H3s3-2 3-9" />
    <path d="M10.3 21a1.94 1.94 0 0 0 3.4 0" />
  </svg>
);

const SparkIcon = ({ className }: { className?: string }) => (
  <svg
    xmlns="http://www.w3.org/2000/svg"
    width="16"
    height="16"
    viewBox="0 0 24 24"
    fill="none"
    stroke="currentColor"
    strokeWidth="2"
    strokeLinecap="round"
    strokeLinejoin="round"
    className={className}
  >
    <path d="M12 2l1.5 6L20 10l-6.5 2L12 18l-1.5-6L4 10l6.5-2L12 2z" />
  </svg>
);

const formatDuration = (secondsRaw: number): string => {
  const seconds = Number.isFinite(secondsRaw) ? Math.max(0, Math.trunc(secondsRaw)) : 0;
  if (seconds <= 0) return "Off";
  const parts: string[] = [];
  let rem = seconds;
  const units: Array<[number, string]> = [
    [86400, "d"],
    [3600, "h"],
    [60, "m"],
    [1, "s"],
  ];
  for (const [unit, label] of units) {
    if (rem < unit) continue;
    const v = Math.floor(rem / unit);
    rem -= v * unit;
    parts.push(`${v}${label}`);
    if (parts.length >= 2) break;
  }
  return parts.join(" ");
};

const Section = ({
  isDark,
  icon: Icon,
  title,
  description,
  children,
}: {
  isDark: boolean;
  icon: React.ElementType;
  title: string;
  description: string;
  children: React.ReactNode;
}) => (
  <div className={cardClass(isDark)}>
    <div className="flex items-center gap-2 mb-1">
      <div className={`p-1.5 rounded-md ${isDark ? "bg-slate-800 text-indigo-400" : "bg-indigo-50 text-indigo-600"}`}>
        <Icon className="w-4 h-4" />
      </div>
      <h3 className={`text-sm font-semibold ${isDark ? "text-slate-100" : "text-gray-900"}`}>{title}</h3>
    </div>
    <p className={`text-xs ml-9 mb-4 ${isDark ? "text-slate-500" : "text-gray-500"}`}>{description}</p>
    <div className="space-y-4 ml-1">{children}</div>
  </div>
);

const NumberInputRow = ({
  label,
  value,
  onChange,
  isDark,
  min = 0,
  helperText,
  formatValue = true,
  onAutoSave,
}: {
  label: string;
  value: number;
  onChange: (val: number) => void;
  isDark: boolean;
  min?: number;
  helperText?: React.ReactNode;
  formatValue?: boolean;
  onAutoSave?: () => void;
}) => (
  <div className="w-full">
    <label className={labelClass(isDark)}>{label}</label>
    <div className="relative">
      <input
        type="number"
        min={min}
        value={value}
        onChange={(e) => onChange(Number(e.target.value))}
        onBlur={() => onAutoSave?.()}
        className={inputClass(isDark)}
      />
      {formatValue ? (
        <div
          className={`
            absolute right-3 top-1/2 -translate-y-1/2 text-xs font-mono
            pointer-events-none transition-opacity duration-200
            ${isDark ? "text-slate-600" : "text-gray-400"}
          `}
        >
          {formatDuration(value)}
        </div>
      ) : null}
    </div>
    {helperText && (
      <div className={`mt-1.5 text-[11px] leading-snug ${isDark ? "text-slate-500" : "text-gray-500"}`}>
        {helperText}
      </div>
    )}
  </div>
);

const Chip = ({
  label,
  onRemove,
  isDark,
}: {
  label: string;
  onRemove?: () => void;
  isDark: boolean;
}) => (
  <span
    className={`inline-flex items-center gap-1 px-2 py-1 rounded-full text-[11px] border ${
      isDark ? "border-slate-700 bg-slate-900 text-slate-200" : "border-gray-200 bg-white text-gray-700"
    }`}
  >
    <span className="font-mono">{label}</span>
    {onRemove ? (
      <button
        type="button"
        onClick={onRemove}
        className={`ml-0.5 rounded-full w-4 h-4 flex items-center justify-center ${
          isDark ? "hover:bg-slate-800 text-slate-300" : "hover:bg-gray-100 text-gray-500"
        }`}
        aria-label={`Remove ${label}`}
      >
        ×
      </button>
    ) : null}
  </span>
);

function clampInt(v: number, min: number, max: number) {
  const n = Number.isFinite(v) ? Math.trunc(v) : min;
  return Math.max(min, Math.min(max, n));
}

function isValidId(id: string) {
  return /^[A-Za-z0-9][A-Za-z0-9_-]{0,63}$/.test(id);
}

function nowId(prefix: string) {
  return `${prefix}_${Date.now().toString(36)}`;
}

function defaultNotifyAction(): Extract<AutomationRuleAction, { kind: "notify" }> {
  return { kind: "notify", priority: "high", requires_ack: false, snippet_ref: null, message: "" };
}

function defaultGroupStateAction(): Extract<AutomationRuleAction, { kind: "group_state" }> {
  return { kind: "group_state", state: "paused" };
}

function defaultActorControlAction(): Extract<AutomationRuleAction, { kind: "actor_control" }> {
  return { kind: "actor_control", operation: "restart", targets: ["@all"] };
}

function actionKind(action: AutomationRule["action"] | undefined): "notify" | "group_state" | "actor_control" {
  const kind = String(action?.kind || "notify").trim();
  if (kind === "group_state" || kind === "actor_control") return kind;
  return "notify";
}

const GROUP_STATE_COPY: Record<"active" | "idle" | "paused" | "stopped", { label: string; hint: string }> = {
  active: { label: "Activate Group", hint: "Start runners if needed, then resume active automation." },
  idle: { label: "Set Idle", hint: "Keep sessions running but disable proactive automation." },
  paused: { label: "Pause Delivery", hint: "Pause automation and notification delivery." },
  stopped: { label: "Stop Group", hint: "Stop all actor runtimes for this group." },
};

const ACTOR_OPERATION_COPY: Record<"start" | "stop" | "restart", { label: string; hint: string }> = {
  start: { label: "Start Runtimes", hint: "Start selected actor runtimes." },
  stop: { label: "Stop Runtimes", hint: "Stop selected actor runtimes." },
  restart: { label: "Restart Runtimes", hint: "Restart selected actor runtimes." },
};

function _localTimeZone(): string {
  try {
    return Intl.DateTimeFormat().resolvedOptions().timeZone || "UTC";
  } catch {
    return "UTC";
  }
}

type SchedulePreset = "daily" | "weekly" | "monthly";

const WEEKDAY_OPTIONS: Array<{ value: number; label: string }> = [
  { value: 1, label: "Mon" },
  { value: 2, label: "Tue" },
  { value: 3, label: "Wed" },
  { value: 4, label: "Thu" },
  { value: 5, label: "Fri" },
  { value: 6, label: "Sat" },
  { value: 0, label: "Sun" },
];

const AUTOMATION_VAR_HELP: Record<string, { description: string; example: string }> = {
  interval_minutes: {
    description: "Minutes for interval schedules (0 when not interval based).",
    example: "15",
  },
  group_title: {
    description: "Current group name.",
    example: "Riichi Arena Ops",
  },
  actor_names: {
    description: "Comma-separated enabled member names.",
    example: "foreman, peer1, peer2",
  },
  scheduled_at: {
    description: "Planned send time in UTC (ISO).",
    example: "2026-02-10T12:00:00Z",
  },
};

function parseCronToPreset(cronExpr: string): { preset: SchedulePreset; hour: number; minute: number; weekday: number; dayOfMonth: number } {
  const raw = String(cronExpr || "").trim();
  const parts = raw.split(/\s+/).filter(Boolean);
  if (parts.length !== 5) {
    return { preset: "daily", hour: 9, minute: 0, weekday: 1, dayOfMonth: 1 };
  }
  const [mStr, hStr, dom, mon, dow] = parts;
  if (!/^\d+$/.test(mStr) || !/^\d+$/.test(hStr)) {
    return { preset: "daily", hour: 9, minute: 0, weekday: 1, dayOfMonth: 1 };
  }
  const minute = clampInt(Number(mStr), 0, 59);
  const hour = clampInt(Number(hStr), 0, 23);

  if (dom === "*" && mon === "*" && dow === "*") {
    return { preset: "daily", hour, minute, weekday: 1, dayOfMonth: 1 };
  }
  if (dom === "*" && mon === "*" && /^\d+$/.test(dow)) {
    const weekdayRaw = Number(dow);
    const weekday = weekdayRaw === 7 ? 0 : clampInt(weekdayRaw, 0, 6);
    return { preset: "weekly", hour, minute, weekday, dayOfMonth: 1 };
  }
  if (/^\d+$/.test(dom) && mon === "*" && dow === "*") {
    const dayOfMonth = clampInt(Number(dom), 1, 31);
    return { preset: "monthly", hour, minute, weekday: 1, dayOfMonth };
  }
  return { preset: "daily", hour, minute, weekday: 1, dayOfMonth: 1 };
}

function buildCronFromPreset(args: { preset: SchedulePreset; hour: number; minute: number; weekday: number; dayOfMonth: number }): string {
  const hour = clampInt(args.hour, 0, 23);
  const minute = clampInt(args.minute, 0, 59);
  const weekday = clampInt(args.weekday, 0, 6);
  const dayOfMonth = clampInt(args.dayOfMonth, 1, 31);
  if (args.preset === "daily") {
    return `${minute} ${hour} * * *`;
  }
  if (args.preset === "weekly") {
    return `${minute} ${hour} * * ${weekday}`;
  }
  return `${minute} ${hour} ${dayOfMonth} * *`;
}

function isoToLocalDatetimeInput(iso: string): string {
  const value = String(iso || "").trim();
  if (!value) return "";
  const dt = new Date(value);
  if (!Number.isFinite(dt.getTime())) return "";
  const yyyy = dt.getFullYear();
  const mm = String(dt.getMonth() + 1).padStart(2, "0");
  const dd = String(dt.getDate()).padStart(2, "0");
  const hh = String(dt.getHours()).padStart(2, "0");
  const mi = String(dt.getMinutes()).padStart(2, "0");
  return `${yyyy}-${mm}-${dd}T${hh}:${mi}`;
}

function localDatetimeInputToIso(input: string): string {
  const raw = String(input || "").trim();
  if (!raw) return "";
  const dt = new Date(raw);
  if (!Number.isFinite(dt.getTime())) return "";
  return dt.toISOString();
}

function formatTimeInput(hour: number, minute: number): string {
  const hh = String(clampInt(hour, 0, 23)).padStart(2, "0");
  const mm = String(clampInt(minute, 0, 59)).padStart(2, "0");
  return `${hh}:${mm}`;
}

function parseTimeInput(input: string): { hour: number; minute: number } {
  const raw = String(input || "").trim();
  const m = /^(\d{1,2}):(\d{1,2})$/.exec(raw);
  if (!m) return { hour: 9, minute: 0 };
  return {
    hour: clampInt(Number(m[1]), 0, 23),
    minute: clampInt(Number(m[2]), 0, 59),
  };
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
  const renderEditingRuleModal = () => {
    if (!editingRule) return null;

    const rid = String(editingRule.id || "").trim();
    const st = editingRuleStatus || {};
    const to = Array.isArray(editingRule.to) ? editingRule.to.map((x) => String(x || "").trim()).filter(Boolean) : [];
    const triggerKind = String(editingRule.trigger?.kind || "interval");
    const every = clampInt(
      Number(triggerKind === "interval" && editingRule.trigger && "every_seconds" in editingRule.trigger ? editingRule.trigger.every_seconds : 0),
      1,
      365 * 24 * 3600
    );
    const cronExpr = String(triggerKind === "cron" && editingRule.trigger && "cron" in editingRule.trigger ? editingRule.trigger.cron : "").trim();
    const atRaw = String(triggerKind === "at" && editingRule.trigger && "at" in editingRule.trigger ? editingRule.trigger.at : "").trim();
    const kind = actionKind(editingRule.action);
    const scheduleLockedToOneTime = kind !== "notify";
    const scheduleSelectValue = scheduleLockedToOneTime ? "at" : triggerKind;
    const activeTriggerKind = scheduleLockedToOneTime ? "at" : triggerKind;
    const operationalActionsEnabled = activeTriggerKind === "at";
    const snippetRef = String(kind === "notify" && editingRule.action && "snippet_ref" in editingRule.action ? editingRule.action.snippet_ref || "" : "").trim();
    const msg = String(kind === "notify" && editingRule.action && "message" in editingRule.action ? editingRule.action.message || "" : "");
    const contentMode: "snippet" | "custom" = snippetRef ? "snippet" : "custom";
    const groupStateValue = String(
      kind === "group_state" && editingRule.action && "state" in editingRule.action ? editingRule.action.state || "paused" : "paused"
    );
    const actorOperation = String(
      kind === "actor_control" && editingRule.action && "operation" in editingRule.action ? editingRule.action.operation || "restart" : "restart"
    );
    const actorTargets = Array.isArray(kind === "actor_control" && editingRule.action && "targets" in editingRule.action ? editingRule.action.targets : [])
      ? (editingRule.action as { targets?: string[] }).targets?.map((x) => String(x || "").trim()).filter(Boolean) || []
      : [];
    const notifyAction = kind === "notify" && editingRule.action && editingRule.action.kind === "notify" ? editingRule.action : defaultNotifyAction();
    const enabled = editingRule.enabled !== false;
    const scope = String(editingRule.scope || "group") === "personal" ? "personal" : "group";
    const ownerActorId = String(editingRule.owner_actor_id || "").trim();
    const localTz = _localTimeZone();
    const schedule = parseCronToPreset(cronExpr);
    const scheduleTime = formatTimeInput(schedule.hour, schedule.minute);
    const atInput = isoToLocalDatetimeInput(atRaw);
    const oneShotMode = oneShotModeByRule[rid] || "exact";
    const oneShotAfterMinutes = clampInt(oneShotAfterMinutesByRule[rid] ?? 30, 1, 7 * 24 * 60);

    return (
      <div
        className="fixed inset-0 z-[1000]"
        role="dialog"
        aria-modal="true"
        onMouseDown={(e) => {
          if (e.target === e.currentTarget) setEditingRuleId(null);
        }}
      >
        <div className="absolute inset-0 bg-black/50" />
        <div
          className={`absolute inset-2 sm:inset-auto sm:left-1/2 sm:top-1/2 sm:w-[min(840px,calc(100vw-20px))] sm:h-[min(78vh,760px)] sm:-translate-x-1/2 sm:-translate-y-1/2 rounded-xl sm:rounded-2xl border ${
            isDark ? "border-slate-800 bg-slate-950" : "border-gray-200 bg-white"
          } shadow-2xl flex flex-col overflow-hidden`}
        >
          <div className={`px-4 py-3 border-b ${isDark ? "border-slate-800" : "border-gray-200"} flex items-start gap-3`}>
            <div className="min-w-0">
              <div className={`text-sm font-semibold ${isDark ? "text-slate-100" : "text-gray-900"}`}>
                Edit Rule: <span className="font-mono">{rid || "unnamed"}</span>
              </div>
              <div className={`mt-1 text-[11px] ${isDark ? "text-slate-400" : "text-gray-600"}`}>
                Last: {st.last_fired_at || "—"} • Next: {st.next_fire_at || "—"} {st.completed ? `• Completed: ${st.completed_at || st.last_fired_at || "—"}` : ""} {st.last_error ? `• Error: ${st.last_error_at || "—"}` : ""}
              </div>
              <div className={`mt-1 text-[11px] ${isDark ? "text-slate-500" : "text-gray-500"}`}>
                Changes apply to draft immediately. Click Save in Automation Rules to persist.
              </div>
            </div>

            <div className="ml-auto flex items-center gap-2 flex-wrap justify-end">
              <label className={`text-xs ${isDark ? "text-slate-400" : "text-gray-600"} flex items-center gap-2`}>
                <input
                  type="checkbox"
                  checked={enabled}
                  onChange={(e) => updateRuleNested(rid, { enabled: e.target.checked })}
                />
                on
              </label>
              <button
                type="button"
                className={`px-3 py-2 rounded-lg text-sm min-h-[44px] transition-colors ${
                  isDark ? "bg-slate-900 hover:bg-slate-800 text-slate-300 border border-slate-800" : "bg-white hover:bg-gray-50 text-gray-700 border border-gray-200"
                }`}
                onClick={() => {
                  removeRule(rid);
                  setEditingRuleId(null);
                }}
              >
                Delete
              </button>
              <button
                type="button"
                className={`px-3 py-2 rounded-lg text-sm min-h-[44px] transition-colors ${
                  isDark ? "bg-slate-900 hover:bg-slate-800 text-slate-300 border border-slate-800" : "bg-white hover:bg-gray-50 text-gray-700 border border-gray-200"
                }`}
                onClick={() => setEditingRuleId(null)}
              >
                Close
              </button>
            </div>
          </div>

          {rulesErr ? <div className={`px-4 pt-3 text-xs ${isDark ? "text-rose-300" : "text-rose-600"}`}>{rulesErr}</div> : null}
          {st.last_error ? <div className={`px-4 pt-1 text-xs ${isDark ? "text-rose-300" : "text-rose-600"}`}>{st.last_error}</div> : null}

          <div className="p-3 sm:p-4 flex-1 overflow-auto space-y-3">
            <div className="grid grid-cols-1 sm:grid-cols-2 gap-3">
              <div>
                <label className={labelClass(isDark)}>Rule Name (ID)</label>
                <input
                  value={rid}
                  onChange={(e) => {
                    const nextId = e.target.value;
                    updateRuleNested(rid, { id: nextId });
                    if (nextId.trim()) setEditingRuleId(nextId.trim());
                  }}
                  className={`${inputClass(isDark)} font-mono`}
                  placeholder="daily_checkin"
                  spellCheck={false}
                />
                <div className={`mt-1 text-[11px] ${isDark ? "text-slate-500" : "text-gray-500"}`}>
                  Unique name (letters, numbers, `_`, `-`).
                </div>
              </div>
              <div>
                <label className={labelClass(isDark)}>Schedule Type</label>
                <select
                  value={scheduleSelectValue}
                  disabled={scheduleLockedToOneTime}
                  onChange={(e) => {
                    const nextKind = String(e.target.value || "interval");
                    if (nextKind === "cron") {
                      const presetCron = buildCronFromPreset({
                        preset: schedule.preset,
                        hour: schedule.hour,
                        minute: schedule.minute,
                        weekday: schedule.weekday,
                        dayOfMonth: schedule.dayOfMonth,
                      });
                      updateRuleNested(rid, {
                        trigger: {
                          kind: "cron",
                          cron: cronExpr || presetCron,
                          timezone: localTz,
                        },
                      });
                      return;
                    }
                    if (nextKind === "at") {
                      setOneShotModeByRule((prev) => ({ ...prev, [rid]: "after" }));
                      setOneShotAfterMinutesByRule((prev) => ({ ...prev, [rid]: 30 }));
                      const defaultAt = atRaw || new Date(Date.now() + 30 * 60 * 1000).toISOString();
                      updateRuleNested(rid, { trigger: { kind: "at", at: defaultAt } });
                      return;
                    }
                    updateRuleNested(rid, { trigger: { kind: "interval", every_seconds: every } });
                  }}
                  className={inputClass(isDark)}
                >
                  {kind === "notify" ? <option value="interval">Interval Schedule</option> : null}
                  {kind === "notify" ? <option value="cron">Recurring Schedule</option> : null}
                  <option value="at">One-Time Schedule</option>
                </select>
                <div className={`mt-1 text-[11px] ${isDark ? "text-slate-500" : "text-gray-500"}`}>
                  {scheduleLockedToOneTime
                    ? "This action supports one-time scheduling only."
                    : activeTriggerKind === "interval"
                    ? "Interval: repeat every N minutes."
                    : activeTriggerKind === "cron"
                      ? "Recurring: run daily, weekly, or monthly."
                      : "One-Time: run once by countdown or exact time."}
                </div>
              </div>
            </div>

            {scope === "personal" ? (
              <div className={`text-[11px] ${isDark ? "text-amber-300" : "text-amber-700"}`}>
                Personal rule (owner: <span className="font-mono">{ownerActorId || "unknown"}</span>). Scope is controlled by permissions.
              </div>
            ) : null}

            {activeTriggerKind === "interval" ? (
              <div className="grid grid-cols-1 sm:grid-cols-2 gap-3">
                <div>
                  <label className={labelClass(isDark)}>Repeat Every (minutes)</label>
                  <input
                    type="number"
                    min={1}
                    value={Math.max(1, Math.round(every / 60))}
                    onChange={(e) =>
                      updateRuleNested(rid, {
                        trigger: {
                          kind: "interval",
                          every_seconds: Math.max(1, Number(e.target.value || 1)) * 60,
                        },
                      })
                    }
                    className={inputClass(isDark)}
                  />
                </div>
                <div className={`self-end text-[11px] ${isDark ? "text-slate-500" : "text-gray-500"}`}>
                  Current cadence: {formatDuration(every)}
                </div>
              </div>
            ) : null}

            {activeTriggerKind === "cron" ? (
              <div className="space-y-3">
                <div className="grid grid-cols-1 sm:grid-cols-2 gap-3">
                  <div>
                    <label className={labelClass(isDark)}>Pattern</label>
                    <select
                      value={schedule.preset}
                      onChange={(e) => {
                        const preset = String(e.target.value || "daily") as SchedulePreset;
                        const nextCron = buildCronFromPreset({
                          preset,
                          hour: schedule.hour,
                          minute: schedule.minute,
                          weekday: schedule.weekday,
                          dayOfMonth: schedule.dayOfMonth,
                        });
                        updateRuleNested(rid, { trigger: { kind: "cron", cron: nextCron, timezone: localTz } });
                      }}
                      className={inputClass(isDark)}
                    >
                      <option value="daily">Daily</option>
                      <option value="weekly">Weekly</option>
                      <option value="monthly">Monthly</option>
                    </select>
                  </div>
                  <div>
                    <label className={labelClass(isDark)}>Time</label>
                    <input
                      type="time"
                      value={scheduleTime}
                      onChange={(e) => {
                        const parsed = parseTimeInput(e.target.value);
                        const nextCron = buildCronFromPreset({
                          preset: schedule.preset,
                          hour: parsed.hour,
                          minute: parsed.minute,
                          weekday: schedule.weekday,
                          dayOfMonth: schedule.dayOfMonth,
                        });
                        updateRuleNested(rid, { trigger: { kind: "cron", cron: nextCron, timezone: localTz } });
                      }}
                      className={inputClass(isDark)}
                    />
                  </div>
                </div>
                {schedule.preset === "weekly" ? (
                  <div>
                    <label className={labelClass(isDark)}>Weekday</label>
                    <select
                      value={String(schedule.weekday)}
                      onChange={(e) => {
                        const nextCron = buildCronFromPreset({
                          preset: "weekly",
                          hour: schedule.hour,
                          minute: schedule.minute,
                          weekday: Number(e.target.value || 1),
                          dayOfMonth: schedule.dayOfMonth,
                        });
                        updateRuleNested(rid, { trigger: { kind: "cron", cron: nextCron, timezone: localTz } });
                      }}
                      className={inputClass(isDark)}
                    >
                      {WEEKDAY_OPTIONS.map((day) => (
                        <option key={day.value} value={String(day.value)}>
                          {day.label}
                        </option>
                      ))}
                    </select>
                  </div>
                ) : null}
                {schedule.preset === "monthly" ? (
                  <div>
                    <label className={labelClass(isDark)}>Day of Month</label>
                    <input
                      type="number"
                      min={1}
                      max={31}
                      value={schedule.dayOfMonth}
                      onChange={(e) => {
                        const nextCron = buildCronFromPreset({
                          preset: "monthly",
                          hour: schedule.hour,
                          minute: schedule.minute,
                          weekday: schedule.weekday,
                          dayOfMonth: Number(e.target.value || 1),
                        });
                        updateRuleNested(rid, { trigger: { kind: "cron", cron: nextCron, timezone: localTz } });
                      }}
                      className={inputClass(isDark)}
                    />
                  </div>
                ) : null}
              </div>
            ) : null}

            {activeTriggerKind === "at" ? (
              <div className="space-y-3">
                <div>
                  <label className={labelClass(isDark)}>One-Time Mode</label>
                  <select
                    value={oneShotMode}
                    onChange={(e) => setOneShotModeByRule((prev) => ({ ...prev, [rid]: String(e.target.value || "after") as "after" | "exact" }))}
                    className={inputClass(isDark)}
                  >
                    <option value="after">After countdown</option>
                    <option value="exact">Exact time</option>
                  </select>
                </div>

                {oneShotMode === "after" ? (
                  <div className="space-y-2">
                    <div className="flex flex-wrap gap-2">
                      {[5, 10, 30, 60, 120].map((mins) => (
                        <button
                          key={mins}
                          type="button"
                          className={`px-2.5 py-1.5 rounded-lg text-xs transition-colors ${
                            isDark ? "bg-slate-800 hover:bg-slate-700 text-slate-200 border border-slate-700" : "bg-white hover:bg-gray-50 text-gray-700 border border-gray-200"
                          }`}
                          onClick={() => setOneShotAfterMinutes(rid, mins)}
                        >
                          {mins >= 60 ? `${Math.round(mins / 60)}h` : `${mins}m`}
                        </button>
                      ))}
                    </div>
                    <div>
                      <label className={labelClass(isDark)}>Remind Me In (minutes)</label>
                      <input
                        type="number"
                        min={1}
                        max={10080}
                        value={oneShotAfterMinutes}
                        onChange={(e) => {
                          const minutes = clampInt(Number(e.target.value || 1), 1, 7 * 24 * 60);
                          setOneShotAfterMinutes(rid, minutes);
                        }}
                        className={inputClass(isDark)}
                      />
                    </div>
                  </div>
                ) : (
                  <div>
                    <label className={labelClass(isDark)}>Date & Time</label>
                    <input
                      type="datetime-local"
                      value={atInput}
                      onChange={(e) => updateRuleNested(rid, { trigger: { kind: "at", at: localDatetimeInputToIso(e.target.value) } })}
                      className={inputClass(isDark)}
                    />
                  </div>
                )}

                <div className={`text-[11px] ${isDark ? "text-slate-500" : "text-gray-500"}`}>
                  Saved send time (UTC): <span className="font-mono break-all">{atRaw || "—"}</span>
                </div>
              </div>
            ) : null}

            <div>
              <label className={labelClass(isDark)}>Action</label>
              <select
                value={kind}
                onChange={(e) => {
                  const next = String(e.target.value || "notify");
                  if (next !== "notify" && !operationalActionsEnabled) {
                    setRulesErr("Set Group Status and Control Actor Runtimes are available only for One-Time schedule.");
                    return;
                  }
                  setRulesErr("");
                  if (next === "group_state") {
                    const defaultAt = atRaw || new Date(Date.now() + 30 * 60 * 1000).toISOString();
                    setOneShotModeByRule((prev) => ({ ...prev, [rid]: "after" }));
                    setOneShotAfterMinutesByRule((prev) => ({ ...prev, [rid]: prev[rid] ?? 30 }));
                    updateRuleNested(rid, { action: defaultGroupStateAction(), trigger: { kind: "at", at: defaultAt } });
                    return;
                  }
                  if (next === "actor_control") {
                    const defaultAt = atRaw || new Date(Date.now() + 30 * 60 * 1000).toISOString();
                    setOneShotModeByRule((prev) => ({ ...prev, [rid]: "after" }));
                    setOneShotAfterMinutesByRule((prev) => ({ ...prev, [rid]: prev[rid] ?? 30 }));
                    updateRuleNested(rid, { action: defaultActorControlAction(), trigger: { kind: "at", at: defaultAt } });
                    return;
                  }
                  updateRuleNested(rid, { action: defaultNotifyAction(), to: to.length > 0 ? to : ["@foreman"] });
                }}
                className={inputClass(isDark)}
              >
                <option value="notify">Send Reminder</option>
                <option value="group_state" disabled={!operationalActionsEnabled}>
                  Set Group Status{operationalActionsEnabled ? "" : " (One-Time only)"}
                </option>
                <option value="actor_control" disabled={!operationalActionsEnabled}>
                  Control Actor Runtimes{operationalActionsEnabled ? "" : " (One-Time only)"}
                </option>
              </select>
              {!operationalActionsEnabled ? (
                <div className={`mt-1 text-[11px] ${isDark ? "text-slate-500" : "text-gray-500"}`}>
                  Operational actions are available only with One-Time schedule.
                </div>
              ) : null}
            </div>

            {kind === "notify" ? (
              <>
                <div>
                  <label className={labelClass(isDark)}>Send To</label>
                  <div className="flex flex-wrap gap-2">
                    {to.map((tok) => (
                      <Chip
                        key={tok}
                        label={tok}
                        isDark={isDark}
                        onRemove={() => updateRuleNested(rid, { to: to.filter((x) => x !== tok) })}
                      />
                    ))}
                    <select
                      value=""
                      onChange={(e) => {
                        const v = String(e.target.value || "").trim();
                        if (!v) return;
                        if (!to.includes(v)) updateRuleNested(rid, { to: [...to, v] });
                      }}
                      className={`px-3 py-2 rounded-lg text-sm min-h-[44px] ${
                        isDark ? "bg-slate-900 text-slate-200 border border-slate-800" : "bg-white text-gray-800 border border-gray-200"
                      }`}
                    >
                      <option value="">+ Add recipient...</option>
                      {actorTargetOptions.map((o) => (
                        <option key={o.value} value={o.value}>
                          {o.label}
                        </option>
                      ))}
                    </select>
                  </div>
                </div>

                <div>
                  <label className={labelClass(isDark)}>Notification Source</label>
                  <select
                    value={contentMode}
                    onChange={(e) => {
                      const nextMode = String(e.target.value || "custom");
                      if (nextMode === "snippet") {
                        if (snippetIds.length === 0) {
                          setRulesErr("Create at least one snippet before selecting Snippet.");
                          return;
                        }
                        setRulesErr("");
                        const nextSnippetRef = snippetRef || snippetIds[0] || "";
                        updateRuleNested(rid, {
                          action: { ...notifyAction, snippet_ref: nextSnippetRef || null },
                        });
                        return;
                      }
                      setRulesErr("");
                      updateRuleNested(rid, { action: { ...notifyAction, snippet_ref: null } });
                    }}
                    className={inputClass(isDark)}
                  >
                    <option value="snippet">Message Snippet</option>
                    <option value="custom">Type text</option>
                  </select>
                </div>

                {contentMode === "snippet" ? (
                  <div>
                    <label className={labelClass(isDark)}>Message Snippet</label>
                    <select
                      value={snippetRef}
                      onChange={(e) =>
                        updateRuleNested(rid, { action: { ...notifyAction, snippet_ref: e.target.value || null } })
                      }
                      className={`${inputClass(isDark)} font-mono`}
                    >
                      <option value="">(select snippet)</option>
                      {snippetIds.map((sid) => (
                        <option key={sid} value={sid}>
                          {sid}
                        </option>
                      ))}
                    </select>
                    {snippetIds.length === 0 ? (
                      <div className={`mt-1 text-[11px] ${isDark ? "text-amber-300" : "text-amber-700"}`}>
                        No snippets yet. Create one in Snippets first.
                      </div>
                    ) : null}
                  </div>
                ) : (
                  <div>
                    <label className={labelClass(isDark)}>Message</label>
                    <textarea
                      value={msg}
                      onChange={(e) => updateRuleNested(rid, { action: { ...notifyAction, message: e.target.value } })}
                      className={`${inputClass(isDark)} font-mono text-[12px]`}
                      style={{ minHeight: 140 }}
                      placeholder="Write the message sent when this rule runs."
                      spellCheck={false}
                    />
                  </div>
                )}
              </>
            ) : null}

            {kind === "group_state" ? (
              <div>
                <label className={labelClass(isDark)}>Group Status Target</label>
                <select
                  value={groupStateValue}
                  onChange={(e) => updateRuleNested(rid, { action: { kind: "group_state", state: String(e.target.value || "paused") as "active" | "idle" | "paused" | "stopped" } })}
                  className={inputClass(isDark)}
                >
                  <option value="active">{GROUP_STATE_COPY.active.label}</option>
                  <option value="idle">{GROUP_STATE_COPY.idle.label}</option>
                  <option value="paused">{GROUP_STATE_COPY.paused.label}</option>
                  <option value="stopped">{GROUP_STATE_COPY.stopped.label}</option>
                </select>
                <div className={`mt-1 text-[11px] ${isDark ? "text-slate-500" : "text-gray-500"}`}>
                  {GROUP_STATE_COPY[(groupStateValue as "active" | "idle" | "paused" | "stopped") || "paused"].hint}
                </div>
              </div>
            ) : null}

            {kind === "actor_control" ? (
              <div className="space-y-3">
                <div>
                  <label className={labelClass(isDark)}>Runtime Operation</label>
                  <select
                    value={actorOperation}
                    onChange={(e) =>
                      updateRuleNested(rid, {
                        action: {
                          kind: "actor_control",
                          operation: String(e.target.value || "restart") as "start" | "stop" | "restart",
                          targets: actorTargets.length > 0 ? actorTargets : ["@all"],
                        },
                      })
                    }
                    className={inputClass(isDark)}
                  >
                    <option value="start">{ACTOR_OPERATION_COPY.start.label}</option>
                    <option value="stop">{ACTOR_OPERATION_COPY.stop.label}</option>
                    <option value="restart">{ACTOR_OPERATION_COPY.restart.label}</option>
                  </select>
                  <div className={`mt-1 text-[11px] ${isDark ? "text-slate-500" : "text-gray-500"}`}>
                    {ACTOR_OPERATION_COPY[(actorOperation as "start" | "stop" | "restart") || "restart"].hint}
                  </div>
                </div>
                <div>
                  <label className={labelClass(isDark)}>Target Actors</label>
                  <div className="flex flex-wrap gap-2">
                    {actorTargets.map((tok) => (
                      <Chip
                        key={tok}
                        label={tok}
                        isDark={isDark}
                        onRemove={() =>
                          updateRuleNested(rid, {
                            action: {
                              kind: "actor_control",
                              operation: actorOperation as "start" | "stop" | "restart",
                              targets: actorTargets.filter((x) => x !== tok),
                            },
                          })
                        }
                      />
                    ))}
                    <select
                      value=""
                      onChange={(e) => {
                        const v = String(e.target.value || "").trim();
                        if (!v) return;
                        if (actorTargets.includes(v)) return;
                        updateRuleNested(rid, {
                          action: {
                            kind: "actor_control",
                            operation: actorOperation as "start" | "stop" | "restart",
                            targets: [...actorTargets, v],
                          },
                        });
                      }}
                      className={`px-3 py-2 rounded-lg text-sm min-h-[44px] ${
                        isDark ? "bg-slate-900 text-slate-200 border border-slate-800" : "bg-white text-gray-800 border border-gray-200"
                      }`}
                    >
                      <option value="">+ Add target...</option>
                      {actorTargetOptions.map((o) => (
                        <option key={o.value} value={o.value}>
                          {o.label}
                        </option>
                      ))}
                    </select>
                  </div>
                </div>
              </div>
            ) : null}
          </div>
        </div>
      </div>
    );
  };

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

        <div className="space-y-2">
          <div className="flex items-center justify-end gap-2 flex-wrap">
            <label
              className={`inline-flex items-center gap-1.5 px-2 py-1.5 rounded-md text-[11px] min-h-[32px] border ${
                isDark ? "border-slate-700 text-slate-300 bg-slate-900" : "border-gray-200 text-gray-700 bg-white"
              }`}
            >
              <input
                type="checkbox"
                checked={showCompletedRules}
                onChange={(e) => setShowCompletedRules(Boolean(e.target.checked))}
                className="h-3 w-3"
              />
              Show Completed
            </label>
            <button
              type="button"
              className={`px-2 py-1.5 rounded-md text-[11px] min-h-[32px] font-medium transition-colors ${
                isDark ? "bg-slate-900 hover:bg-slate-800 text-slate-200 border border-slate-700" : "bg-white hover:bg-gray-50 text-gray-700 border border-gray-200"
              } disabled:opacity-50`}
              onClick={clearCompletedRules}
              disabled={rulesBusy || completedOneTimeRuleIds.length === 0}
              title="Clear completed one-time reminders"
            >
              Clear Completed ({completedOneTimeRuleIds.length})
            </button>
          </div>

          {visibleRules.length === 0 ? (
            <div className={`text-sm ${isDark ? "text-slate-400" : "text-gray-600"}`}>No rules yet. Create one or reset to defaults.</div>
          ) : null}

          {visibleRules.map((r) => {
            const rid = String(r.id || "").trim();
            const st = status[rid] || {};
            const to = Array.isArray(r.to) ? r.to.map((x) => String(x || "").trim()).filter(Boolean) : [];
            const triggerKind = String(r.trigger?.kind || "interval");
            const every = clampInt(
              Number(triggerKind === "interval" && r.trigger && "every_seconds" in r.trigger ? r.trigger.every_seconds : 0),
              1,
              365 * 24 * 3600
            );
            const cronExpr = String(triggerKind === "cron" && r.trigger && "cron" in r.trigger ? r.trigger.cron : "").trim();
            const atRaw = String(triggerKind === "at" && r.trigger && "at" in r.trigger ? r.trigger.at : "").trim();
            const kind = actionKind(r.action);
            const snippetRef = String(kind === "notify" && r.action && "snippet_ref" in r.action ? r.action.snippet_ref || "" : "").trim();
            const msg = String(kind === "notify" && r.action && "message" in r.action ? r.action.message || "" : "").trim();
            const enabled = r.enabled !== false;
            const nextFireAt = String(st.next_fire_at || "").trim();
            const lastFireAt = String(st.last_fired_at || "").trim();
            const schedule = parseCronToPreset(cronExpr);
            const scheduleTime = formatTimeInput(schedule.hour, schedule.minute);
            const weekdayLabel = WEEKDAY_OPTIONS.find((x) => x.value === schedule.weekday)?.label || String(schedule.weekday);
            const atLocal = atRaw ? isoToLocalDatetimeInput(atRaw).replace("T", " ") : "";
            let scheduleLabel = "Schedule not set";
            if (triggerKind === "interval") {
              scheduleLabel = `Every ${Math.max(1, Math.round(every / 60))} min`;
            } else if (triggerKind === "cron") {
              if (schedule.preset === "daily") scheduleLabel = `Daily ${scheduleTime}`;
              else if (schedule.preset === "weekly") scheduleLabel = `Weekly ${weekdayLabel} ${scheduleTime}`;
              else scheduleLabel = `Monthly day ${schedule.dayOfMonth} ${scheduleTime}`;
            } else if (triggerKind === "at") {
              scheduleLabel = atLocal ? `One-time ${atLocal}` : "One-time (time not set)";
            }
            let actionLabel = "Action not set";
            if (kind === "notify") {
              const contentLabel = snippetRef ? `Snippet: ${snippetRef}` : msg ? "Typed message" : "Message not set";
              const recipientsLabel = to.length > 0 ? to.join(", ") : "(no recipients)";
              actionLabel = `Reminder -> ${recipientsLabel} • ${contentLabel}`;
            } else if (kind === "group_state") {
              const stateValue = String(r.action && "state" in r.action ? r.action.state || "paused" : "paused");
              const normalizedState = (["active", "idle", "paused", "stopped"].includes(stateValue)
                ? stateValue
                : "paused") as "active" | "idle" | "paused" | "stopped";
              actionLabel = `Group status -> ${GROUP_STATE_COPY[normalizedState].label}`;
            } else if (kind === "actor_control") {
              const operation = String(r.action && "operation" in r.action ? r.action.operation || "restart" : "restart");
              const targets = Array.isArray(r.action && "targets" in r.action ? r.action.targets : [])
                ? (r.action as { targets?: string[] }).targets?.map((x) => String(x || "").trim()).filter(Boolean) || []
                : [];
              const normalizedOperation = (["start", "stop", "restart"].includes(operation)
                ? operation
                : "restart") as "start" | "stop" | "restart";
              actionLabel = `${ACTOR_OPERATION_COPY[normalizedOperation].label} -> ${targets.length > 0 ? targets.join(", ") : "(no targets)"}`;
            }
            const hasError = Boolean(st.last_error);
            const completed = Boolean(st.completed) && triggerKind === "at";
            const completedAt = String(st.completed_at || "").trim();

            return (
              <div key={rid || nowId("rule")} className={cardClass(isDark)}>
                <div className="flex items-start justify-between gap-3">
                  <div className="min-w-0 flex-1">
                    <div className={`text-sm font-semibold ${isDark ? "text-slate-100" : "text-gray-900"}`}>{rid || "Rule"}</div>
                    <div className={`mt-0.5 text-[11px] ${isDark ? "text-slate-500" : "text-gray-500"}`}>
                      {scheduleLabel} • {enabled ? "On" : "Off"} {completed ? "• Completed" : ""}
                    </div>
                  </div>

                  <div className="flex items-center gap-2 shrink-0">
                    <label className={`text-xs ${isDark ? "text-slate-400" : "text-gray-600"} flex items-center gap-2`}>
                      <input
                        type="checkbox"
                        checked={enabled}
                        onChange={(e) => updateRuleNested(rid, { enabled: e.target.checked })}
                      />
                      on
                    </label>
                    <button
                      type="button"
                      className={`px-3 py-2 rounded-lg text-xs min-h-[36px] transition-colors ${
                        isDark ? "bg-slate-900 hover:bg-slate-800 text-slate-300 border border-slate-800" : "bg-white hover:bg-gray-50 text-gray-700 border border-gray-200"
                      }`}
                      onClick={() => {
                        setEditingRuleId(rid);
                        setRulesErr("");
                      }}
                    >
                      Edit
                    </button>
                    <button
                      type="button"
                      className={`px-3 py-2 rounded-lg text-xs min-h-[36px] transition-colors ${
                        isDark ? "bg-slate-900 hover:bg-slate-800 text-slate-300 border border-slate-800" : "bg-white hover:bg-gray-50 text-gray-700 border border-gray-200"
                      }`}
                      onClick={() => removeRule(rid)}
                      title="Delete rule"
                    >
                      Delete
                    </button>
                  </div>
                </div>

                <div className={`mt-2 text-[11px] ${isDark ? "text-slate-500" : "text-gray-500"} break-words`}>
                  <span className="font-mono">{actionLabel}</span>
                </div>
                <div className={`mt-1 text-[11px] ${isDark ? "text-slate-500" : "text-gray-500"}`}>
                  Last: {lastFireAt || "—"} • Next: {nextFireAt || "—"}
                </div>
                {completed ? (
                  <div className={`mt-1 text-[11px] ${isDark ? "text-emerald-300" : "text-emerald-700"}`}>
                    Completed at: {completedAt || lastFireAt || "—"}
                  </div>
                ) : null}
                {hasError ? (
                  <div className={`mt-1 text-[11px] break-words ${isDark ? "text-rose-300" : "text-rose-600"}`}>{st.last_error}</div>
                ) : null}
              </div>
            );
          })}
        </div>

        <div className={`mt-2 text-[11px] ${isDark ? "text-slate-500" : "text-gray-500"}`}>
          Edit opens full settings in a modal to keep this list clean. Message snippets are managed in Snippets.
        </div>
      </Section>

      {renderEditingRuleModal()}

      <Section
        isDark={isDark}
        icon={BellIcon}
        title="Engine Policies"
        description="Built-in follow-ups and alerts. Adjust values, then click Save Policies."
      >
        <NumberInputRow
          isDark={isDark}
          label="Unread Follow-up (sec)"
          value={props.nudgeSeconds}
          onChange={props.setNudgeSeconds}
          helperText="Remind a member when unread messages sit too long."
        />

        <div className="grid grid-cols-1 sm:grid-cols-2 gap-4">
          <NumberInputRow
            isDark={isDark}
            label="Need Reply Follow-up (sec)"
            value={props.replyRequiredNudgeSeconds}
            onChange={props.setReplyRequiredNudgeSeconds}
            helperText="For messages marked Need Reply."
          />
          <NumberInputRow
            isDark={isDark}
            label="Important Follow-up (sec)"
            value={props.attentionAckNudgeSeconds}
            onChange={props.setAttentionAckNudgeSeconds}
            helperText="For important messages awaiting acknowledgement."
          />
        </div>

        <div className="grid grid-cols-1 sm:grid-cols-2 gap-4">
          <NumberInputRow
            isDark={isDark}
            label="Backlog Digest Follow-up (sec)"
            value={props.unreadNudgeSeconds}
            onChange={props.setUnreadNudgeSeconds}
            helperText="For regular unread backlog digests."
          />
          <NumberInputRow
            isDark={isDark}
            label="Digest Minimum Gap (sec)"
            value={props.nudgeDigestMinIntervalSeconds}
            onChange={props.setNudgeDigestMinIntervalSeconds}
            helperText="Minimum gap between digests for the same member."
          />
        </div>

        <div className="grid grid-cols-1 sm:grid-cols-2 gap-4">
          <NumberInputRow
            isDark={isDark}
            label="Max Repeats Per Item"
            value={props.nudgeMaxRepeatsPerObligation}
            onChange={props.setNudgeMaxRepeatsPerObligation}
            formatValue={false}
            helperText="Maximum follow-ups for one pending item."
          />
          <NumberInputRow
            isDark={isDark}
            label="Escalate To Foreman After"
            value={props.nudgeEscalateAfterRepeats}
            onChange={props.setNudgeEscalateAfterRepeats}
            formatValue={false}
            helperText="Escalate when repeat count reaches this value."
          />
        </div>

        <div className="grid grid-cols-1 sm:grid-cols-2 gap-4">
          <NumberInputRow
            isDark={isDark}
            label="Keepalive Delay (sec)"
            value={props.keepaliveSeconds}
            onChange={props.setKeepaliveSeconds}
            helperText="Wait time after an actor says 'Next:'."
          />
          <NumberInputRow
            isDark={isDark}
            label="Keepalive Max Retries"
            value={props.keepaliveMax}
            onChange={props.setKeepaliveMax}
            formatValue={false}
            helperText={props.keepaliveMax <= 0 ? "Infinite retries" : `Retry up to ${props.keepaliveMax} times`}
          />
        </div>

        <div className="grid grid-cols-1 sm:grid-cols-2 gap-4">
          <NumberInputRow
            isDark={isDark}
            label="Help Refresh Interval (sec)"
            value={props.helpNudgeIntervalSeconds}
            onChange={props.setHelpNudgeIntervalSeconds}
            helperText="Time since last help follow-up."
          />
          <NumberInputRow
            isDark={isDark}
            label="Help Refresh Min Msgs"
            value={props.helpNudgeMinMessages}
            onChange={props.setHelpNudgeMinMessages}
            formatValue={false}
            helperText="Minimum accumulated messages."
          />
        </div>
        <div className={`pt-2 text-xs font-semibold ${isDark ? "text-slate-300" : "text-gray-700"}`}>Foreman Alerts</div>
        <NumberInputRow
          isDark={isDark}
          label="Actor Idle Alert (sec)"
          value={props.idleSeconds}
          onChange={props.setIdleSeconds}
          helperText="Alert foreman if actor is inactive for this long."
        />

        <NumberInputRow
          isDark={isDark}
          label="Group Silence Check (sec)"
          value={props.silenceSeconds}
          onChange={props.setSilenceSeconds}
          helperText="Alert foreman if the entire group is silent."
        />
        <div className="pt-2 flex items-center justify-end">
          <button onClick={props.onSavePolicies} disabled={props.busy} className={`${primaryButtonClass(props.busy)} w-full sm:w-auto`} title="Save engine policy settings">
            {props.busy ? "Saving..." : "Save Policies"}
          </button>
        </div>
      </Section>

      {snippetManagerOpen ? (
        <div
          className="fixed inset-0 z-[1000]"
          role="dialog"
          aria-modal="true"
          onMouseDown={(e) => {
            if (e.target === e.currentTarget) closeSnippetManager();
          }}
        >
          <div className="absolute inset-0 bg-black/50" />
          <div
            className={`absolute inset-2 sm:inset-auto sm:left-1/2 sm:top-1/2 sm:w-[min(820px,calc(100vw-20px))] sm:h-[min(74vh,700px)] sm:-translate-x-1/2 sm:-translate-y-1/2 rounded-xl sm:rounded-2xl border ${
              isDark ? "border-slate-800 bg-slate-950" : "border-gray-200 bg-white"
            } shadow-2xl flex flex-col overflow-hidden`}
          >
            <div className={`px-4 py-3 border-b ${isDark ? "border-slate-800" : "border-gray-200"} flex items-start gap-3`}>
              <div className="min-w-0">
                <div className={`text-sm font-semibold ${isDark ? "text-slate-100" : "text-gray-900"}`}>
                  Snippets
                </div>
                <div className={`mt-1 text-[11px] ${isDark ? "text-slate-400" : "text-gray-600"}`}>
                  Reusable notification messages for automation rules.
                </div>
              </div>
              <button
                type="button"
                className={`ml-auto px-3 py-2 rounded-lg text-sm min-h-[44px] transition-colors ${
                  isDark ? "bg-slate-900 hover:bg-slate-800 text-slate-300 border border-slate-800" : "bg-white hover:bg-gray-50 text-gray-700 border border-gray-200"
                }`}
                onClick={closeSnippetManager}
              >
                Close
              </button>
            </div>

            <div className="p-3 sm:p-4 flex-1 overflow-auto space-y-3">
              {templateErr ? <div className={`text-xs ${isDark ? "text-rose-300" : "text-rose-600"}`}>{templateErr}</div> : null}
              <div className="grid grid-cols-1 sm:grid-cols-[1fr_auto] gap-2">
                <input
                  value={newSnippetId}
                  onChange={(e) => setNewSnippetId(e.target.value)}
                  className={`${inputClass(isDark)} font-mono`}
                  placeholder="snippet_name"
                  spellCheck={false}
                />
                <button
                  type="button"
                  className={`px-3 py-2 rounded-lg text-sm min-h-[44px] font-medium transition-colors ${
                    isDark ? "bg-slate-800 hover:bg-slate-700 text-slate-200 border border-slate-700" : "bg-white hover:bg-gray-50 text-gray-800 border border-gray-200"
                  }`}
                  onClick={addSnippet}
                >
                  + Add Snippet
                </button>
              </div>

              {supportedVars.length > 0 ? (
                <div className={`rounded-lg border p-2.5 text-[11px] ${isDark ? "border-slate-800 bg-slate-900/60 text-slate-400" : "border-gray-200 bg-gray-50 text-gray-600"}`}>
                  <div className={`font-semibold mb-1 ${isDark ? "text-slate-300" : "text-gray-700"}`}>Available placeholders</div>
                  <div className="space-y-1">
                    {supportedVars.map((v) => {
                      const help = AUTOMATION_VAR_HELP[v];
                      return (
                        <div key={v}>
                          <span className="font-mono">{`{{${v}}}`}</span>
                          <span>{` - ${help?.description || "Built-in placeholder."}`}</span>
                          <span className={isDark ? "text-slate-500" : "text-gray-500"}>{` (example: ${help?.example || "-"})`}</span>
                        </div>
                      );
                    })}
                  </div>
                </div>
              ) : null}

              {snippetIds.length === 0 ? (
                <div className={`text-sm ${isDark ? "text-slate-400" : "text-gray-600"}`}>No snippets yet.</div>
              ) : null}

              <div className="space-y-3">
                {snippetIds.map((sid) => (
                  <div key={sid} className={cardClass(isDark)}>
                    <div className="flex items-center justify-between gap-2 mb-2">
                      <div className={`text-xs font-semibold font-mono ${isDark ? "text-slate-200" : "text-gray-800"}`}>{sid}</div>
                      <button
                        type="button"
                        className={`px-2 py-1.5 rounded-lg text-xs min-h-[36px] transition-colors ${
                          isDark ? "bg-slate-900 hover:bg-slate-800 text-slate-300 border border-slate-800" : "bg-white hover:bg-gray-50 text-gray-700 border border-gray-200"
                        }`}
                        onClick={() => deleteSnippet(sid)}
                        title="Delete snippet"
                      >
                        Delete
                      </button>
                    </div>
                    <textarea
                      value={draft.snippets[sid] || ""}
                      onChange={(e) => updateSnippet(sid, e.target.value)}
                      className={`${inputClass(isDark)} font-mono text-[12px]`}
                      style={{ minHeight: 140 }}
                      spellCheck={false}
                    />
                  </div>
                ))}
              </div>
            </div>
          </div>
        </div>
      ) : null}
    </div>
  );
}
