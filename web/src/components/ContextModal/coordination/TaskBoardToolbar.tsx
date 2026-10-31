import { useLayoutEffect, useRef, type ReactNode } from "react";
import { classNames } from "../../../utils/classNames";
import { useLcdStyle } from "../../../utils/uiStyle";
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
  actions,
  jump,
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
  /** LCD only: the board's own actions, kept on the toolbar's row. */
  actions?: ReactNode;
  /** LCD only: column navigation, held in the same sticky layer as the filters. */
  jump?: ReactNode;
  onTaskQueryChange: (value: string) => void;
  onAssigneeFilterChange: (value: string) => void;
  onTaskFilterChange: (value: TaskFilterValue) => void;
  onClearFilters: () => void;
  onArchivedExpandedChange: (value: boolean) => void;
}) {
  const lcd = useLcdStyle();
  const filters: Array<[TaskFilterValue, string, number]> = [
    ["all", tr("context.all", "All"), Number(tasksSummary.total || 0)],
    ["blocked", tr("context.blocked", "Blocked"), attentionCounts.blocked],
    ["waiting_user", tr("context.waitingUser", "Waiting user"), attentionCounts.waitingUser],
    ["handoff", tr("context.pendingHandoffs", "Pending handoffs"), attentionCounts.pendingHandoffs],
    ["unassigned", tr("context.unassigned", "Unassigned"), unassignedCount],
  ];
  // The counts cover every task, so a search or assignee filter would make
  // them misstate the board. A filter with no tasks still has none then.
  const countsApply = !taskQuery.trim() && assigneeFilter === "__all__";
  const filtered = Boolean(
    taskQuery.trim() || assigneeFilter !== "__all__" || taskFilter !== "all",
  );

  const searchInput = (
    <input
      value={taskQuery}
      onChange={(event) => onTaskQueryChange(event.target.value)}
      className={ui.inputClass}
      aria-label={tr("context.searchTasks", "Search tasks by title, id, assignee, or outcome")}
      placeholder={tr("context.searchTasksShort", "Search tasks")}
    />
  );
  const assigneeSelect = (
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
  );
  const filterButtons = filters
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
        {countsApply ? `${label} · ${count}` : label}
      </button>
    ));
  const syncing = syncBusy ? (
    <span className={classNames("text-xs italic", ui.mutedTextClass)}>
      {tr("context.applyingChanges", "Applying changes…")}
    </span>
  ) : null;
  const matching =
    typeof matchingCount === "number" ? (
      <p className={classNames("text-xs", ui.mutedTextClass)} role="status">
        {tr("context.matchingTasks", "{{count}} matching tasks", { count: matchingCount })}
      </p>
    ) : null;

  const stickyRef = useRef<HTMLDivElement>(null);
  const jumpLayerRef = useRef<HTMLDivElement>(null);
  const hasJumpLayer = Boolean(jump || syncing || matching);
  // Keyboard focus has to scroll clear of the pinned layer. Wrapping, the text
  // size, the touch keys' pixel floor and the status line all change its height,
  // so the scroller gets the measured height, which the stylesheet pads by.
  useLayoutEffect(() => {
    const sticky = stickyRef.current;
    const scroll = sticky?.closest<HTMLElement>("[data-context-scroll]");
    if (!sticky || !scroll) return;
    const jumpLayer = jumpLayerRef.current;
    const update = () => {
      // Below 768px the outer layer has no box and only the jump layer is pinned.
      const height = Math.max(
        sticky.getBoundingClientRect().height,
        jumpLayer?.getBoundingClientRect().height ?? 0,
      );
      scroll.style.setProperty("--taskboard-pinned-height", `${Math.ceil(height)}px`);
    };
    update();
    const observer = new ResizeObserver(update);
    observer.observe(sticky, { box: "border-box" });
    if (jumpLayer) observer.observe(jumpLayer, { box: "border-box" });
    return () => {
      observer.disconnect();
      scroll.style.removeProperty("--taskboard-pinned-height");
    };
  }, [lcd, hasJumpLayer]);

  if (lcd) {
    // One sticky layer: search and the new-task key, then the filters, which a
    // phone scrolls sideways. Clear comes first there so it stays in view. On a
    // phone only the column jump and the status stay pinned, as their own layer.
    return (
      <div ref={stickyRef} data-taskboard-sticky className="flex flex-col gap-2">
        <div data-taskboard-toolbar>
          <div data-taskboard-search>{searchInput}</div>
          <div data-taskboard-controls>
            {filtered ? (
              <button
                type="button"
                onClick={onClearFilters}
                className={ui.buttonSecondaryClass}
                data-taskboard-clear
              >
                {tr("context.clearFilters", "Clear filters")}
              </button>
            ) : null}
            <div data-taskboard-assignee>{assigneeSelect}</div>
            <div data-taskboard-filters>{filterButtons}</div>
            {hasArchivedTasks ? (
              <button
                type="button"
                aria-pressed={archivedExpanded}
                onClick={() => onArchivedExpandedChange(!archivedExpanded)}
                className={ui.chipBaseClass}
                data-taskboard-archive
              >
                {tr("context.showArchived", "Show archived")}
              </button>
            ) : null}
          </div>
          {actions}
        </div>
        {jump || syncing || matching ? (
          <div ref={jumpLayerRef} data-taskboard-jump-layer className="flex flex-col gap-2">
            {jump}
            {syncing || matching ? (
              <div className="flex flex-wrap items-center gap-x-3">
                {syncing}
                {matching}
              </div>
            ) : null}
          </div>
        ) : null}
      </div>
    );
  }

  return (
    <div className="flex flex-col gap-3">
      <div className="grid flex-1 grid-cols-[minmax(0,1fr)_minmax(0,1fr)] gap-2 lg:grid-cols-[minmax(0,1fr)_14rem_auto]">
        {searchInput}
        {assigneeSelect}
        {filtered ? (
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
        {filterButtons}
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
        {syncing}
      </div>
      {matching}
    </div>
  );
}
