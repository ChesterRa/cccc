import { useState } from "react";
import type { CSSProperties } from "react";
import type { AgentState } from "../../../types";
import { formatFullTime, formatTime } from "../../../utils/time";
import { classNames } from "../../../utils/classNames";
import {
  agentHot,
  agentWarm,
  type ContextTranslator,
  hasMindContext,
  hasRecoveryCues,
  isAgentStale,
  recoverySummary,
} from "../model";

function clampTextStyle(lines: number): CSSProperties {
  return {
    display: "-webkit-box",
    WebkitBoxOrient: "vertical",
    WebkitLineClamp: lines,
    overflow: "hidden",
  };
}

interface ExpandableTextBlockProps {
  label: string;
  text: string;
  mutedTextClass: string;
  subtleTextClass: string;
  tr: ContextTranslator;
  lines?: number;
}

function ExpandableTextBlock({
  label,
  text,
  mutedTextClass,
  subtleTextClass,
  tr,
  lines = 4,
}: ExpandableTextBlockProps) {
  const [expanded, setExpanded] = useState(false);
  const value = String(text || "").trim();
  if (!value) return null;

  const lineCount = value.split(/\r?\n/).length;
  const needsToggle = value.length > 240 || lineCount > lines;

  return (
    <div>
      <div className={classNames("text-[11px] font-medium uppercase tracking-wide", mutedTextClass)}>{label}</div>
      <div
        className={classNames("mt-1 text-sm leading-6 whitespace-pre-wrap break-words", subtleTextClass)}
        style={!expanded && needsToggle ? clampTextStyle(lines) : undefined}
      >
        {value}
      </div>
      {needsToggle ? (
        <button
          type="button"
          onClick={() => setExpanded((prev) => !prev)}
          aria-expanded={expanded}
          className={classNames(
            "mt-2 text-xs font-medium transition-colors",
            "text-[rgb(35,36,37)] hover:opacity-75 dark:text-white"
          )}
        >
          {expanded ? tr("context.showLess", "Show less") : tr("context.showMore", "Show more")}
        </button>
      ) : null}
    </div>
  );
}

interface AgentStateCardProps {
  agent: AgentState;
  tr: ContextTranslator;
  mutedTextClass: string;
  subtleTextClass: string;
}

export function AgentStateCard({ agent, tr, mutedTextClass, subtleTextClass }: AgentStateCardProps) {
  const [recoveryOpen, setRecoveryOpen] = useState(false);
  const hot = agentHot(agent);
  const warm = agentWarm(agent);
  const stale = isAgentStale(agent);
  const executionEmpty = !(hot.focus || hot.nextAction || hot.activeTaskId || hot.blockers.length > 0);
  const mindContextEmpty = !hasMindContext(agent);
  const recoveryEmpty = !hasRecoveryCues(agent);
  const sectionClass = classNames("rounded-xl border px-3 py-3", "glass-card");
  const sectionTitleClass = classNames("text-xs font-semibold uppercase tracking-[0.12em]", "text-[var(--color-text-secondary)]");
  const summary = recoverySummary(agent, tr);

  return (
    <article className={classNames("rounded-2xl border p-4", "glass-card")}>
      <div className="flex items-start justify-between gap-3">
        <div className="min-w-0">
          <div className={classNames("text-sm font-semibold break-words", "text-[var(--color-text-primary)]")}>{agent.id}</div>
          <div className="mt-1 flex flex-wrap items-center gap-2">
            <span
              className={classNames("text-xs", stale ? "text-amber-600 dark:text-amber-300" : mutedTextClass)}
              title={agent.updated_at ? formatFullTime(agent.updated_at) : undefined}
            >
              {agent.updated_at ? tr("context.updated", "Updated {{time}}", { time: formatTime(agent.updated_at) }) : tr("context.notUpdatedYet", "Not updated yet")}
            </span>
            {stale ? (
              <span className={classNames("rounded-full px-2 py-0.5 text-[11px]", "bg-amber-500/15 text-amber-700 dark:text-amber-300")}>
                {tr("context.stale", "Stale")}
              </span>
            ) : null}
          </div>
        </div>
        <div className="flex flex-wrap items-center justify-end gap-2">
          {hot.activeTaskId ? (
            <span className={classNames("rounded-full px-2 py-0.5 text-[11px]", "border border-black/10 bg-[rgb(245,245,245)] text-[rgb(35,36,37)] dark:border-white/12 dark:bg-white/[0.08] dark:text-white")}>
              {hot.activeTaskId}
            </span>
          ) : null}
          {hot.blockers.length > 0 ? (
            <span className={classNames("rounded-full px-2 py-0.5 text-[11px]", "bg-rose-500/15 text-rose-600 dark:text-rose-400")}>
              {tr("context.blocked", "Blocked")}
            </span>
          ) : null}
        </div>
      </div>

      <section className={classNames(sectionClass, "mt-4")}>
        <div className={sectionTitleClass}>{tr("context.executionNow", "Execution Now")}</div>
        {executionEmpty ? (
          <div className={classNames("mt-2 text-sm", mutedTextClass)}>{tr("context.noExecutionState", "No active execution state")}</div>
        ) : (
          <div className="mt-3 space-y-3">
            <ExpandableTextBlock label={tr("context.focus", "Focus")} text={hot.focus} mutedTextClass={mutedTextClass} subtleTextClass={subtleTextClass} tr={tr} lines={3} />
            <ExpandableTextBlock label={tr("context.nextAction", "Next action")} text={hot.nextAction} mutedTextClass={mutedTextClass} subtleTextClass={subtleTextClass} tr={tr} lines={3} />
            {hot.activeTaskId ? (
              <div>
                <div className={classNames("text-[11px] font-medium uppercase tracking-wide", mutedTextClass)}>{tr("context.activeTask", "Active task")}</div>
                <div className={classNames("mt-1 text-sm break-words", subtleTextClass)}>{hot.activeTaskId}</div>
              </div>
            ) : null}
            {hot.blockers.length > 0 ? (
              <div className={classNames("rounded-xl border px-3 py-3", "border-rose-500/30 bg-rose-500/10")}>
                <div className={classNames("text-[11px] font-medium uppercase tracking-wide", "text-rose-600 dark:text-rose-300")}>
                  {tr("context.blockers", "Blockers")}
                </div>
                <ul className={classNames("mt-2 space-y-1 text-sm list-disc pl-5", "text-rose-700 dark:text-rose-200")}>
                  {hot.blockers.map((blocker, index) => <li key={`${agent.id}-blocker-${index}`}>{blocker}</li>)}
                </ul>
              </div>
            ) : null}
          </div>
        )}
      </section>

      <section className={classNames(sectionClass, "mt-3")}>
        <div className={sectionTitleClass}>{tr("context.mindContext", "Mind Context")}</div>
        {mindContextEmpty ? (
          <div className={classNames("mt-2 text-sm", mutedTextClass)}>{tr("context.noMindContext", "No mind context recorded yet")}</div>
        ) : (
          <div className="mt-3 space-y-4">
            <ExpandableTextBlock label={tr("context.environmentContext", "Environment Context")} text={warm.environmentSummary} mutedTextClass={mutedTextClass} subtleTextClass={subtleTextClass} tr={tr} />
            <ExpandableTextBlock label={tr("context.userPreferences", "User Preferences")} text={warm.userModel} mutedTextClass={mutedTextClass} subtleTextClass={subtleTextClass} tr={tr} />
            <ExpandableTextBlock label={tr("context.workingStance", "Working Stance")} text={warm.personaNotes} mutedTextClass={mutedTextClass} subtleTextClass={subtleTextClass} tr={tr} />
          </div>
        )}
      </section>

      <section className={classNames(sectionClass, "mt-3")}>
        <button type="button" onClick={() => setRecoveryOpen((prev) => !prev)} aria-expanded={recoveryOpen} className="flex w-full items-start justify-between gap-3 text-left">
          <div className="min-w-0">
            <div className={sectionTitleClass}>{tr("context.recoveryCues", "Recovery Cues")}</div>
            <div className={classNames("mt-1 text-xs break-words", mutedTextClass)}>{summary}</div>
          </div>
          <span className={classNames("mt-0.5 text-sm transition-transform", mutedTextClass, recoveryOpen ? "rotate-180" : "")} aria-hidden="true">
            ▾
          </span>
        </button>
        {recoveryOpen ? (
          recoveryEmpty ? (
            <div className={classNames("mt-3 text-sm", mutedTextClass)}>{tr("context.noRecoveryCues", "No recovery cues")}</div>
          ) : (
            <div className="mt-3 space-y-4">
              <ExpandableTextBlock label={tr("context.whatChanged", "What changed")} text={warm.whatChanged} mutedTextClass={mutedTextClass} subtleTextClass={subtleTextClass} tr={tr} lines={3} />
              {warm.openLoops.length > 0 ? (
                <div>
                  <div className={classNames("text-[11px] font-medium uppercase tracking-wide", mutedTextClass)}>{tr("context.openLoops", "Open loops")}</div>
                  <ul className={classNames("mt-2 space-y-1 text-sm list-disc pl-5", subtleTextClass)}>
                    {warm.openLoops.map((item, index) => <li key={`${agent.id}-open-loop-${index}`}>{item}</li>)}
                  </ul>
                </div>
              ) : null}
              {warm.commitments.length > 0 ? (
                <div>
                  <div className={classNames("text-[11px] font-medium uppercase tracking-wide", mutedTextClass)}>{tr("context.commitments", "Commitments")}</div>
                  <ul className={classNames("mt-2 space-y-1 text-sm list-disc pl-5", subtleTextClass)}>
                    {warm.commitments.map((item, index) => <li key={`${agent.id}-commitment-${index}`}>{item}</li>)}
                  </ul>
                </div>
              ) : null}
            </div>
          )
        ) : null}
      </section>
    </article>
  );
}
