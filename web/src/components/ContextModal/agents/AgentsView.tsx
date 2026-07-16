import type { AgentState } from "../../../types";
import { classNames } from "../../../utils/classNames";
import { agentHot, type ContextTranslator } from "../model";
import type { ContextModalUi } from "../ui";
import { AgentStateCard } from "./AgentStateCard";

interface AgentsViewProps {
  agents: AgentState[];
  tr: ContextTranslator;
  ui: ContextModalUi;
}

export function AgentsView({ agents, tr, ui }: AgentsViewProps) {
  const agentsWithBlockers = agents.filter((agent) => agentHot(agent).blockers.length > 0).length;
  const agentsWithActiveTask = agents.filter((agent) => !!agentHot(agent).activeTaskId).length;

  return (
    <section className={classNames(ui.surfaceClass, "p-4")}>
      <div className="flex flex-wrap items-start justify-between gap-3">
        <div>
          <div className={classNames("text-lg font-semibold", "text-[var(--color-text-primary)]")}>
            {tr("context.agents", "Agents")}
          </div>
          <div className={classNames("mt-1 text-sm", ui.subtleTextClass)}>
            {tr(
              "context.agentsHint",
              "Use this view to recover each agent’s current execution state, not to steer the whole project.",
            )}
          </div>
        </div>
        <div className="flex flex-wrap items-center gap-2">
          <span
            className={classNames(
              "rounded-full px-2.5 py-1 text-xs",
              "glass-panel text-[var(--color-text-secondary)]",
            )}
          >
            {tr("context.totalAgents", "{{count}} agents", { count: agents.length })}
          </span>
          <span
            className={classNames(
              "rounded-full px-2.5 py-1 text-xs",
              "border border-black/10 bg-[rgb(245,245,245)] text-[rgb(35,36,37)] dark:border-white/12 dark:bg-white/[0.08] dark:text-white",
            )}
          >
            {tr("context.activeTasksCount", "{{count}} with active task", {
              count: agentsWithActiveTask,
            })}
          </span>
          {agentsWithBlockers > 0 ? (
            <span
              className={classNames(
                "rounded-full px-2.5 py-1 text-xs",
                "bg-rose-500/15 text-rose-600 dark:text-rose-400",
              )}
            >
              {tr("context.blockersCount", "{{count}} blockers", { count: agentsWithBlockers })}
            </span>
          ) : null}
        </div>
      </div>
      <div className="mt-4 grid gap-3 2xl:grid-cols-2">
        {agents.length > 0 ? (
          agents.map((agent) => (
            <AgentStateCard
              key={agent.id}
              agent={agent}
              tr={tr}
              mutedTextClass={ui.mutedTextClass}
              subtleTextClass={ui.subtleTextClass}
            />
          ))
        ) : (
          <div
            className={classNames(
              "rounded-xl border border-dashed px-3 py-4 text-sm",
              "border-[var(--glass-border-subtle)] text-[var(--color-text-muted)]",
            )}
          >
            {tr("context.noAgents", "No agent state")}
          </div>
        )}
      </div>
    </section>
  );
}
