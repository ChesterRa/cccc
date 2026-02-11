import { useState } from "react";

import { cardClass } from "./modals/settings/types";

type TemplatePromptKind = "preamble" | "help";

type TemplatePromptPreview = {
  source?: "builtin" | "home";
  chars?: number;
  preview?: string;
};

type TemplateActor = {
  id?: string;
  title?: string;
  runtime?: string;
  runner?: string;
  command?: unknown;
  submit?: string;
  enabled?: boolean;
};

type TemplateSettings = Record<string, unknown>;

type TemplatePayload = {
  v?: number;
  cccc_version?: string;
  title?: string;
  topic?: string;
  exported_at?: string;
  actors?: TemplateActor[];
  settings?: TemplateSettings;
  automation?: { rules?: number; snippets?: number };
  guidance?: Record<string, TemplatePromptPreview>;
  // Legacy: older servers returned this as "prompts".
  prompts?: Record<string, TemplatePromptPreview>;
};

type TemplateDiff = {
  actors_add?: string[];
  actors_update?: string[];
  actors_remove?: string[];
  settings_changed?: Record<string, { from: unknown; to: unknown }>;
  guidance_changed?: Record<
    string,
    {
      changed: boolean;
      current_chars?: number;
      new_chars?: number;
      current_source?: "builtin" | "home";
      new_source?: "builtin" | "home";
    }
  >;
  // Legacy: older servers returned this as "prompts_changed".
  prompts_changed?: Record<
    string,
    {
      changed: boolean;
      current_chars?: number;
      new_chars?: number;
      current_source?: "builtin" | "home";
      new_source?: "builtin" | "home";
    }
  >;
};

export interface TemplatePreviewDetailsProps {
  isDark: boolean;
  template: TemplatePayload;
  diff?: TemplateDiff | null;
  wrap?: boolean;
  detailsOpenByDefault?: boolean;
}

function asStringArray(v: unknown): string[] {
  if (!Array.isArray(v)) return [];
  return v.map((x) => String(x || "").trim()).filter((s) => s);
}

function formatCommand(cmd: unknown): string {
  if (Array.isArray(cmd)) {
    return cmd.map((x) => String(x || "").trim()).filter((s) => s).join(" ");
  }
  return String(cmd || "").trim();
}

export function TemplatePreviewDetails({
  isDark,
  template,
  diff,
  wrap = true,
  detailsOpenByDefault = false,
}: TemplatePreviewDetailsProps) {
  const [detailsOpen, setDetailsOpen] = useState(Boolean(detailsOpenByDefault));

  const actors = Array.isArray(template.actors) ? template.actors : [];
  const settings = (template.settings && typeof template.settings === "object" ? template.settings : {}) as TemplateSettings;
  const guidance = (template.guidance && typeof template.guidance === "object" ? template.guidance : {}) as Record<
    string,
    TemplatePromptPreview
  >;
  const promptsLegacy = (template.prompts && typeof template.prompts === "object" ? template.prompts : {}) as Record<
    string,
    TemplatePromptPreview
  >;
  const guidancePrompts = Object.keys(guidance).length > 0 ? guidance : promptsLegacy;

  const addIds = asStringArray(diff?.actors_add);
  const updateIds = asStringArray(diff?.actors_update);
  const removeIds = asStringArray(diff?.actors_remove);

  const promptMeta = (kind: TemplatePromptKind) => {
    const p = guidancePrompts[kind] || {};
    const changedMap = diff?.guidance_changed || diff?.prompts_changed;
    const ch = changedMap?.[kind];
    const changed = typeof ch?.changed === "boolean" ? ch.changed : undefined;
    const currentChars = ch?.current_chars;
    const newChars = ch?.new_chars;
    const currentSource = ch?.current_source;
    const newSource = ch?.new_source;
    return { p, changed, currentChars, newChars, currentSource, newSource };
  };

  const formatPromptLabel = (kind: TemplatePromptKind) => {
    if (kind === "help") return "CCCC_HELP.md";
    return "CCCC_PREAMBLE.md";
  };

  const formatSource = (s: unknown): string => {
    if (s === "home") return "override";
    if (s === "builtin") return "builtin";
    return "";
  };

  const settingsChangedKeys = diff?.settings_changed ? Object.keys(diff.settings_changed) : [];
  const promptOverrideCount = (["preamble", "help"] as const).filter((k) => guidancePrompts[k]?.source === "home").length;
  const automationRules = Number(template.automation?.rules || 0);
  const automationSnippets = Number(template.automation?.snippets || 0);
  const hasAutomation = automationRules > 0 || automationSnippets > 0;

  const formatSettingLine = (key: string) => {
    if (diff?.settings_changed && diff.settings_changed[key]) {
      const row = diff.settings_changed[key];
      return (
        <div key={key} className={`text-xs ${isDark ? "text-slate-400" : "text-gray-600"}`}>
          <span className="font-mono">{key}</span>: <span className="font-mono">{String(row.from)}</span> →{" "}
          <span className="font-mono">{String(row.to)}</span>
        </div>
      );
    }
    return (
      <div key={key} className={`text-xs ${isDark ? "text-slate-400" : "text-gray-600"}`}>
        <span className="font-mono">{key}</span>: <span className="font-mono">{String(settings[key])}</span>
      </div>
    );
  };

  const stableSettingsKeys: string[] = [
    "default_send_to",
    "nudge_after_seconds",
    "reply_required_nudge_after_seconds",
    "attention_ack_nudge_after_seconds",
    "unread_nudge_after_seconds",
    "nudge_digest_min_interval_seconds",
    "nudge_max_repeats_per_obligation",
    "nudge_escalate_after_repeats",
    "actor_idle_timeout_seconds",
    "keepalive_delay_seconds",
    "keepalive_max_per_actor",
    "silence_timeout_seconds",
    "help_nudge_interval_seconds",
    "help_nudge_min_messages",
    "min_interval_seconds",
    "auto_mark_on_delivery",
    "terminal_transcript_visibility",
    "terminal_transcript_notify_tail",
    "terminal_transcript_notify_lines",
  ].filter((k) => k in settings || (diff?.settings_changed && k in diff.settings_changed));

  const body = (
    <>
      <div className={`text-sm font-semibold ${isDark ? "text-slate-200" : "text-gray-800"}`}>Blueprint preview</div>
      <div className={`text-xs mt-1 ${isDark ? "text-slate-500" : "text-gray-600"}`}>
        Blueprint title/topic are informational only (not applied automatically).
      </div>

      <div className="mt-3 space-y-1">
        <div className={`text-xs ${isDark ? "text-slate-400" : "text-gray-600"}`}>
          Blueprint:{" "}
          <span className="font-mono">
            v{String(template.v || "")} {String(template.cccc_version || "")}
          </span>
        {template.title ? (
          <span className="ml-2">
            • <span className="font-mono">{String(template.title)}</span>
          </span>
        ) : null}
      </div>

        {diff ? (
          <div className={`text-xs ${isDark ? "text-slate-400" : "text-gray-600"}`}>
            Actors: +{addIds.length} / ~{updateIds.length} / -{removeIds.length} • Settings changes:{" "}
            {settingsChangedKeys.length} • Guidance changes:{" "}
            {diff.guidance_changed || diff.prompts_changed
              ? Object.keys(diff.guidance_changed || diff.prompts_changed || {}).filter(
                  (k) => (diff.guidance_changed || diff.prompts_changed || {})[k]?.changed
                ).length
              : 0}
            {hasAutomation ? (
              <>
                {" "}
                • Automation: {automationRules} rule(s) / {automationSnippets} snippet(s)
              </>
            ) : null}
          </div>
        ) : (
          <div className={`text-xs ${isDark ? "text-slate-400" : "text-gray-600"}`}>
            Actors: {actors.length} • Settings: {stableSettingsKeys.length} keys • Guidance:{" "}
            {promptOverrideCount > 0 ? `${promptOverrideCount} override(s)` : "builtin"}
            {hasAutomation ? ` • Automation: ${automationRules} rule(s) / ${automationSnippets} snippet(s)` : ""}
          </div>
        )}
      </div>

      <details
        className={`mt-3 rounded-lg border px-3 py-2 ${
          isDark ? "border-slate-800 bg-slate-950/30" : "border-gray-200 bg-gray-50"
        }`}
        open={detailsOpen}
        onToggle={(e) => setDetailsOpen(e.currentTarget.open)}
      >
        <summary className={`cursor-pointer text-xs font-medium ${isDark ? "text-slate-300" : "text-gray-700"}`}>
          Preview details
        </summary>

        <div className="mt-2 space-y-3">
          {(addIds.length || updateIds.length || removeIds.length) && diff ? (
            <div className="space-y-1">
              {addIds.length ? (
                <div className={`text-xs ${isDark ? "text-slate-400" : "text-gray-600"}`}>
                  Add: <span className="font-mono">{addIds.slice(0, 12).join(", ")}</span>
                  {addIds.length > 12 ? ` (+${addIds.length - 12} more)` : ""}
                </div>
              ) : null}
              {updateIds.length ? (
                <div className={`text-xs ${isDark ? "text-slate-400" : "text-gray-600"}`}>
                  Update: <span className="font-mono">{updateIds.slice(0, 12).join(", ")}</span>
                  {updateIds.length > 12 ? ` (+${updateIds.length - 12} more)` : ""}
                </div>
              ) : null}
              {removeIds.length ? (
                <div className={`text-xs ${isDark ? "text-slate-400" : "text-gray-600"}`}>
                  Remove: <span className="font-mono">{removeIds.slice(0, 12).join(", ")}</span>
                  {removeIds.length > 12 ? ` (+${removeIds.length - 12} more)` : ""}
                </div>
              ) : null}
            </div>
          ) : null}

          {actors.length > 0 ? (
            <div>
              <div className={`text-xs font-medium ${isDark ? "text-slate-300" : "text-gray-700"}`}>Actors</div>
              <div className="mt-1 space-y-1">
                {actors.slice(0, 20).map((a) => {
                  const id = String(a.id || "").trim();
                  if (!id) return null;
                  const title = String(a.title || "").trim();
                  const rt = String(a.runtime || "").trim();
                  const runner = String(a.runner || "").trim();
                  const enabled = a.enabled === false ? "disabled" : "enabled";
                  const cmd = formatCommand(a.command);
                  return (
                    <div key={id} className={`text-xs ${isDark ? "text-slate-400" : "text-gray-600"}`}>
                      <span className="font-mono">{id}</span>
                      {title ? <span className="ml-2">• {title}</span> : null}
                      {rt ? <span className="ml-2 font-mono">• {rt}</span> : null}
                      {runner ? <span className="ml-2 font-mono">• {runner}</span> : null}
                      <span className="ml-2">• {enabled}</span>
                      {cmd ? <div className="mt-1 font-mono text-[11px] opacity-90">{cmd}</div> : null}
                    </div>
                  );
                })}
                {actors.length > 20 ? (
                  <div className={`text-[11px] ${isDark ? "text-slate-500" : "text-gray-500"}`}>
                    …and {actors.length - 20} more
                  </div>
                ) : null}
              </div>
            </div>
          ) : null}

          {stableSettingsKeys.length > 0 ? (
            <div>
              <div className={`text-xs font-medium ${isDark ? "text-slate-300" : "text-gray-700"}`}>Settings</div>
              <div className="mt-1 space-y-1">
                {stableSettingsKeys.slice(0, 12).map((k) => formatSettingLine(k))}
                {stableSettingsKeys.length > 12 ? (
                  <div className={`text-[11px] ${isDark ? "text-slate-500" : "text-gray-500"}`}>
                    …and {stableSettingsKeys.length - 12} more
                  </div>
                ) : null}
              </div>
            </div>
          ) : null}

          <div>
            <div className={`text-xs font-medium ${isDark ? "text-slate-300" : "text-gray-700"}`}>Guidance</div>
            {(["preamble", "help"] as const).map((kind) => {
              const meta = promptMeta(kind);
              const label = formatPromptLabel(kind);
              const changed =
                typeof meta.changed === "boolean" ? meta.changed : undefined;
              const changedLabel =
                changed === undefined ? "" : changed ? "(changed)" : "(unchanged)";
              const srcLabel = (() => {
                const cur = formatSource(meta.currentSource);
                const nxt = formatSource(meta.newSource);
                if (cur && nxt) return `${cur} → ${nxt}`;
                if (meta.p.source === "builtin") return "builtin";
                if (meta.p.source === "home") return "override";
                return "";
              })();
              const charLabel = (() => {
                if (typeof meta.currentChars === "number" || typeof meta.newChars === "number") {
                  return `${String(meta.currentChars ?? "")} → ${String(meta.newChars ?? "")} chars`;
                }
                if (typeof meta.p.chars === "number") return `${String(meta.p.chars)} chars`;
                return "";
              })();
              const summaryMeta = [srcLabel, charLabel].filter(Boolean).join(" • ");
              return (
                <details key={kind} className="mt-2">
                  <summary className={`cursor-pointer text-xs ${isDark ? "text-slate-400" : "text-gray-600"}`}>
                    {label} {changedLabel}{" "}
                    <span className="ml-2 text-[11px] opacity-80">
                      {summaryMeta}
                    </span>
                  </summary>
                  {meta.p.preview ? (
                    <pre
                      className={`mt-2 p-2 rounded overflow-x-auto whitespace-pre text-[11px] ${
                        isDark ? "bg-slate-900 text-slate-200" : "bg-white text-gray-800 border border-gray-200"
                      }`}
                    >
                      {String(meta.p.preview)}
                    </pre>
                  ) : meta.p.source === "builtin" ? (
                    <div className={`mt-2 text-[11px] ${isDark ? "text-slate-500" : "text-gray-500"}`}>
                      Uses built-in defaults (no override file).
                    </div>
                  ) : null}
                </details>
              );
            })}
          </div>
        </div>
      </details>
    </>
  );

  if (!wrap) return <div>{body}</div>;

  return <div className={cardClass(isDark)}>{body}</div>;
}
