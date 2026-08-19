import { classNames } from "../../../utils/classNames";
import { SelectCombobox } from "../../SelectCombobox";
import type { ContextTranslator, TaskFilterValue } from "../model";
import type { ContextModalUi } from "../ui";

interface AttentionCounts {
  blocked: number;
  waitingUser: number;
  pendingHandoffs: number;
}

export function TaskBoardToolbar({
  tr,
  ui,
  syncBusy,
  taskQuery,
  assigneeFilter,
  assigneeOptions,
  taskFilter,
  tasksSummary,
  attentionCounts,
  unassignedCount,
  hasArchivedTasks,
  archivedExpanded,
  onTaskQueryChange,
  onAssigneeFilterChange,
  onTaskFilterChange,
  onClearFilters,
  onArchivedExpandedChange,
}: {
  tr: ContextTranslator;
  ui: ContextModalUi;
  syncBusy: boolean;
  taskQuery: string;
  assigneeFilter: string;
  assigneeOptions: string[];
  taskFilter: TaskFilterValue;
  tasksSummary: { total?: number };
  attentionCounts: AttentionCounts;
  unassignedCount: number;
  hasArchivedTasks: boolean;
  archivedExpanded: boolean;
  onTaskQueryChange: (value: string) => void;
  onAssigneeFilterChange: (value: string) => void;
  onTaskFilterChange: (value: TaskFilterValue) => void;
  onClearFilters: () => void;
  onArchivedExpandedChange: (value: boolean) => void;
}) {
  const filters: Array<[TaskFilterValue, string, number]> = [
    ["all", tr("context.all", "All"), Number(tasksSummary.total || 0)],
    ["blocked", tr("context.blocked", "Blocked"), attentionCounts.blocked],
    ["waiting_user", tr("context.waitingUser", "Waiting user"), attentionCounts.waitingUser],
    ["handoff", tr("context.pendingHandoffs", "Pending handoffs"), attentionCounts.pendingHandoffs],
    ["unassigned", tr("context.unassigned", "Unassigned"), unassignedCount],
  ];

  return (
    <div className="flex flex-col gap-3 border-t border-[var(--glass-border-subtle)] pt-4">
      <div className="grid flex-1 gap-3 lg:grid-cols-[minmax(0,1fr)_auto_auto]">
        <input
          value={taskQuery}
          onChange={(event) => onTaskQueryChange(event.target.value)}
          className={ui.inputClass}
          placeholder={tr("context.searchTasks", "Search tasks by title, id, assignee, or outcome")}
        />
        <SelectCombobox
          items={[
            { value: "__all__", label: tr("context.allAssignees", "All assignees") },
            { value: "__unassigned__", label: tr("context.unassignedOnly", "Unassigned only") },
            ...assigneeOptions.map((assignee) => ({ value: assignee, label: assignee })),
          ]}
          value={assigneeFilter}
          onChange={onAssigneeFilterChange}
          ariaLabel={tr("context.assignee", "Assignee")}
          className={classNames(ui.inputClass, "w-full lg:w-[14rem]")}
          searchable
        />
        <button type="button" onClick={onClearFilters} className={ui.buttonSecondaryClass}>
          {tr("context.clearFilters", "Clear filters")}
        </button>
      </div>

      <div className="flex flex-wrap items-center gap-2">
        {filters.map(([value, label, count]) => (
          <button
            key={value}
            type="button"
            onClick={() => onTaskFilterChange(value)}
            className={classNames(
              ui.chipBaseClass,
              taskFilter === value
                ? "border-black/10 bg-[rgb(35,36,37)] text-white shadow-[0_10px_24px_-20px_rgba(15,23,42,0.3)] dark:border-white/12 dark:bg-white dark:text-[rgb(20,20,22)]"
                : "",
            )}
          >
            {label} · {count}
          </button>
        ))}
        {hasArchivedTasks ? (
          <div className="flex items-center gap-2 sm:ml-auto">
            <span className="text-sm font-medium text-[var(--color-text-primary)]">
              {tr("context.showArchived", "Show archived")}
            </span>
            <button
              type="button"
              role="switch"
              aria-checked={archivedExpanded}
              aria-label={tr("context.showArchived", "Show archived")}
              onClick={() => onArchivedExpandedChange(!archivedExpanded)}
              className={ui.switchTrackClass(archivedExpanded)}
            >
              <span className={ui.switchThumbClass(archivedExpanded)} />
            </button>
          </div>
        ) : null}
        {syncBusy ? (
          <span className={classNames("text-xs italic", ui.mutedTextClass)}>
            {tr("context.applyingChanges", "Applying changes…")}
          </span>
        ) : null}
      </div>
    </div>
  );
}
