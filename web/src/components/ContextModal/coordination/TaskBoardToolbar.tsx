import { classNames } from "../../../utils/classNames";
import { SelectCombobox } from "../../SelectCombobox";
import { Switch } from "../../ui/switch";
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
  matchingCount,
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
  matchingCount?: number | null;
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
    <div className="flex flex-col gap-3">
      <div className="grid flex-1 grid-cols-[minmax(0,1fr)_minmax(0,1fr)] gap-2 lg:grid-cols-[minmax(0,1fr)_14rem_auto]">
        <input
          value={taskQuery}
          onChange={(event) => onTaskQueryChange(event.target.value)}
          className={ui.inputClass}
          aria-label={tr("context.searchTasks", "Search tasks by title, id, assignee, or outcome")}
          placeholder={tr("context.searchTasksShort", "Search tasks")}
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
          className={classNames(ui.inputClass, "w-full px-2 lg:w-[14rem] lg:px-4")}
          searchable
        />
        {taskQuery.trim() || assigneeFilter !== "__all__" || taskFilter !== "all" ? (
          <button
            type="button"
            onClick={onClearFilters}
            className={classNames(ui.buttonSecondaryClass, "col-span-2 lg:col-span-1")}
          >
            {tr("context.clearFilters", "Clear filters")}
          </button>
        ) : null}
      </div>

      <div className="flex flex-wrap items-center gap-2">
        {filters
          .filter(([value, , count]) => value === "all" || count > 0 || taskFilter === value)
          .map(([value, label, count]) => (
            <button
              key={value}
              type="button"
              aria-pressed={taskFilter === value}
              onClick={() => onTaskFilterChange(value)}
              className={classNames(
                ui.chipBaseClass,
                taskFilter === value
                  ? "border-transparent bg-[var(--primary)] text-[var(--primary-foreground)]"
                  : "border-[var(--glass-border-subtle)] text-[var(--color-text-secondary)] hover:bg-[var(--glass-tab-bg-hover)]",
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
            <Switch
              checked={archivedExpanded}
              aria-label={tr("context.showArchived", "Show archived")}
              onChange={(event) => onArchivedExpandedChange(event.currentTarget.checked)}
            />
          </div>
        ) : null}
        {syncBusy ? (
          <span className={classNames("text-xs italic", ui.mutedTextClass)}>
            {tr("context.applyingChanges", "Applying changes…")}
          </span>
        ) : null}
      </div>
      {typeof matchingCount === "number" ? (
        <p className={classNames("text-xs", ui.mutedTextClass)} role="status">
          {tr("context.matchingTasks", "{{count}} matching tasks", { count: matchingCount })}
        </p>
      ) : null}
    </div>
  );
}
