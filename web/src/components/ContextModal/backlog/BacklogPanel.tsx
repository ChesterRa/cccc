import { useCallback, useEffect, useId, useRef, useState } from "react";
import type { BacklogSnapshot, BacklogSource, Task } from "../../../types";
import type { BacklogOperation } from "../../../services/api";
import { classNames } from "../../../utils/classNames";
import { newClientId } from "../../../utils/clientId";
import { ChevronDownIcon, ChevronRightIcon, PlusIcon, RefreshIcon } from "../../Icons";
import { SelectCombobox } from "../../SelectCombobox";
import type { ContextTranslator } from "../model";
import type { ContextModalUi } from "../ui";
import { BacklogEntryItem } from "./BacklogEntryItem";
import { BacklogSetup } from "./BacklogSetup";
import {
  addTargets,
  appendParagraph,
  backlogErrorText,
  backlogFormDirty,
  defaultSection,
  entryKey,
  entryRef,
  fileRef,
  findEntryById,
  firstLine,
  formSelector,
  formTarget,
  moveTargets,
  problemCounts,
  sameFile,
  savedEntryId,
  sectionLabel,
  sourceInFile,
  taskResultParagraph,
  todayLocal,
  type BacklogForm,
  type BacklogIntent,
  type EntryForm,
} from "./backlogModel";
import type { BacklogState } from "./useBacklog";

/** Whether the page knows the Group's project folder yet, and whether it has one. */
export type BacklogScopeState = "loading" | "none" | "ready";

type Located = NonNullable<ReturnType<typeof findEntryById>>;
type Written = { entryId: string; taskId: string; reused: boolean };

function entryDomId(prefix: string, key: string): string {
  return `${prefix}-${key.replace(/[^a-zA-Z0-9_-]/g, "-")}`;
}

export function BacklogPanel({
  backlog,
  scopeState,
  form,
  setForm,
  intent,
  onIntentDone,
  isDark,
  tr,
  ui,
  onOpenTask,
  onTaskPromoted,
  onLinkSource,
  onArchiveTask,
}: {
  backlog: BacklogState;
  scopeState: BacklogScopeState;
  /** The open draft. It is kept by the page, so switching views does not lose it. */
  form: BacklogForm | null;
  setForm: (update: (current: BacklogForm | null) => BacklogForm | null) => void;
  intent: BacklogIntent | null;
  onIntentDone: () => void;
  isDark: boolean;
  tr: ContextTranslator;
  ui: ContextModalUi;
  onOpenTask: (taskId: string) => void;
  /** A task was made from an entry, or found for it: show it. */
  onTaskPromoted: (taskId: string) => Promise<void>;
  /**
   * Records the entry a task came from, unless it already names another. Resolves to the task
   * as linked, or to an error text; an empty one means the page moved on and nothing is left
   * to say.
   */
  onLinkSource: (
    taskId: string,
    source: BacklogSource,
  ) => Promise<{ task: Task } | { error: string }>;
  /** Archives the task as it was when asked; resolves to an error text, empty when done. */
  onArchiveTask: (task: Task) => Promise<string>;
}) {
  const { snapshot, loading, error: loadError, reload, run } = backlog;
  const domPrefix = useId();
  const addTitleId = useId();
  const addBodyId = useId();
  const [expanded, setExpanded] = useState<ReadonlySet<string>>(() => new Set());
  const [closedOpen, setClosedOpen] = useState(false);
  const [busy, setBusy] = useState(false);
  const [failure, setFailure] = useState("");
  const [notice, setNotice] = useState("");
  const [returning, setReturning] = useState<{ entryId: string; task: Task } | null>(null);
  const [archiveAsk, setArchiveAsk] = useState<Task | null>(null);
  // An entry saved for a task that could not be linked to it: only the link is tried again.
  const [linkFailed, setLinkFailed] = useState<{
    task: Task;
    source: BacklogSource;
    error: string;
  } | null>(null);
  const [detailsOpen, setDetailsOpen] = useState(false);
  const [switching, setSwitching] = useState(false);
  const [scrollTo, setScrollTo] = useState("");
  // A save can return after the page moved to another Group or folder and replaced this view.
  // The draft the page holds then belongs to the new view, so the late answer leaves it alone.
  const mounted = useRef(true);
  useEffect(() => {
    mounted.current = true;
    return () => {
      mounted.current = false;
    };
  }, []);

  const reveal = useCallback((located: Located) => {
    if (located.section.closed) setClosedOpen(true);
    const key = entryKey(located.entry);
    setExpanded((current) => (current.has(key) ? current : new Set(current).add(key)));
    setScrollTo(key);
  }, []);

  // Opening a draft over one with unsaved text asks first.
  const replaceForm = useCallback(
    (next: BacklogForm) => {
      if (
        backlogFormDirty(form) &&
        typeof window !== "undefined" &&
        !window.confirm(tr("context.backlogDiscardDraft", "Discard your unsaved backlog draft?"))
      ) {
        return false;
      }
      setFailure("");
      setForm(() => next);
      return true;
    },
    [form, setForm, tr],
  );

  // A new file version carries an open draft along when its entry is still there unchanged.
  // Otherwise the draft stays on the version it saw: it is shown for copying and cannot be
  // written over a change it never saw. A move has nothing to copy, so it just ends.
  useEffect(() => {
    if (!snapshot) return;
    setForm((current) => {
      if (!current || current.kind === "add" || current.sha === snapshot.sha256) return current;
      if (formTarget(snapshot, current)) return { ...current, sha: snapshot.sha256 };
      return current.kind === "move" ? null : current;
    });
  }, [setForm, snapshot]);

  useEffect(() => {
    if (!scrollTo) return;
    const item = document.getElementById(entryDomId(domPrefix, scrollTo));
    item?.scrollIntoView?.({ block: "nearest" });
    // A result added on opening goes at the end of a text that can be longer than its box.
    const box = item?.querySelector("textarea");
    if (box && form?.kind === "edit" && form.body !== form.baseBody) {
      box.scrollTop = box.scrollHeight;
    }
    setScrollTo("");
  }, [domPrefix, form, scrollTo, snapshot]);

  const openEdit = useCallback(
    (snapshotNow: BacklogSnapshot, located: Located, paragraph: string, putBack: Task | null) => {
      const { entry } = located;
      const opened = replaceForm({
        ...entryRef(located, snapshotNow),
        kind: "edit",
        title: entry.title,
        body: paragraph ? appendParagraph(entry.body, paragraph) : entry.body,
        baseTitle: entry.title,
        baseBody: entry.body,
        putBack,
      });
      if (opened) reveal(located);
      return opened;
    },
    [replaceForm, reveal],
  );

  // What the task side asked for is done once, against the file as last read. A task that
  // names an entry goes back only to that entry, in the file it names.
  useEffect(() => {
    if (!intent) return;
    if (!snapshot) {
      if (loadError && !loading) onIntentDone();
      return;
    }
    onIntentDone();
    setNotice("");
    const missing = () =>
      setNotice(
        tr("context.backlogEntryMissing", "That entry is not in the current backlog file."),
      );
    if (intent.kind === "show") {
      const located = sourceInFile(snapshot, intent.source)
        ? findEntryById(snapshot, intent.source.entry_id)
        : null;
      if (located) reveal(located);
      else missing();
      return;
    }
    if (!snapshot.configured || !snapshot.exists) {
      setNotice(tr("context.backlogErrorNotConfigured", "No backlog file is chosen yet."));
      return;
    }
    const task = intent.task;
    if (task.source) {
      const located = sourceInFile(snapshot, task.source)
        ? findEntryById(snapshot, task.source.entry_id)
        : null;
      if (!located) {
        missing();
      } else if (intent.kind === "return") {
        setReturning({ entryId: task.source.entry_id, task });
        reveal(located);
      } else {
        openEdit(snapshot, located, taskResultParagraph(task, todayLocal()), task);
      }
      return;
    }
    if (intent.kind === "return") {
      missing();
      return;
    }
    const paragraph = taskResultParagraph(task, todayLocal());
    replaceForm({
      ...fileRef(snapshot),
      kind: "add",
      section: defaultSection(snapshot),
      title: task.title || "",
      body: paragraph ? `${paragraph}\n` : "",
      putBack: task,
    });
  }, [intent, loadError, loading, onIntentDone, openEdit, replaceForm, reveal, snapshot, tr]);

  // An entry without an ID gets one when it is first written; its key follows.
  const followEntry = useCallback((draft: BacklogForm, entryId: string) => {
    if (!entryId || draft.kind === "add" || draft.entryId) return;
    setExpanded((current) => {
      const next = new Set(current);
      next.delete(`line:${draft.line}`);
      next.add(`id:${entryId}`);
      return next;
    });
  }, []);

  const write = useCallback(
    async (
      operation: BacklogOperation,
      body: Record<string, unknown>,
      draft: BacklogForm,
    ): Promise<Written | null> => {
      setBusy(true);
      setFailure("");
      setNotice("");
      setArchiveAsk(null);
      const response = await run(operation, {
        ...body,
        scope_key: draft.scopeKey,
        scope_url: draft.scopeUrl,
        path: draft.path,
      });
      if (!mounted.current) return null;
      setBusy(false);
      if (response.ok) {
        const entryId = response.result.entry_id || "";
        followEntry(draft, entryId);
        setForm(() => null);
        return {
          entryId,
          taskId: response.result.task_id || "",
          reused: Boolean(response.result.reused),
        };
      }
      setFailure(backlogErrorText(response.error, tr));
      // The file holds what this write saved; only what came after is unconfirmed.
      const saved = savedEntryId(response.error);
      if (!saved) return null;
      if (draft.kind === "promote") {
        // Saving gave the entry its ID. Trying again finds the same entry and request.
        setForm((current) =>
          current?.kind === "promote" && current.requestId === draft.requestId
            ? { ...current, entryId: saved }
            : current,
        );
        return null;
      }
      followEntry(draft, saved);
      setForm(() => null);
      return { entryId: saved, taskId: "", reused: false };
    },
    [followEntry, run, setForm, tr],
  );

  // A task back in the backlog: link it to a new entry, then ask about archiving it.
  const finishPutBack = useCallback(
    async (task: Task, source: BacklogSource | null) => {
      let current = task;
      if (source) {
        setLinkFailed(null);
        setBusy(true);
        const linked = await onLinkSource(task.id, source);
        setBusy(false);
        if ("error" in linked) {
          if (linked.error) setLinkFailed({ task, source, error: linked.error });
          return;
        }
        current = linked.task;
      }
      if (current.status !== "archived") setArchiveAsk(current);
    },
    [onLinkSource],
  );

  const submit = useCallback(async () => {
    if (!form) return;
    const draft = form;
    if (draft.kind === "add") {
      const written = await write(
        "add",
        { section: draft.section, title: draft.title.trim(), body: draft.body },
        draft,
      );
      if (!written?.entryId) return;
      const key = `id:${written.entryId}`;
      setExpanded((current) => new Set(current).add(key));
      setScrollTo(key);
      setNotice(tr("context.backlogAdded", "Entry added."));
      if (draft.putBack) {
        await finishPutBack(draft.putBack, {
          scope_key: draft.scopeKey,
          path: draft.path,
          entry_id: written.entryId,
        });
      }
      return;
    }
    if (draft.kind === "move") return;
    const guard = { entry: formSelector(draft), if_file_sha256: draft.sha };
    if (draft.kind === "edit") {
      const written = await write(
        "edit",
        { ...guard, title: draft.title.trim(), body: draft.body },
        draft,
      );
      if (!written) return;
      setNotice(tr("context.backlogSaved", "Entry saved."));
      // Only a task that names this entry is put back by editing it, so there is nothing to link.
      if (draft.putBack) await finishPutBack(draft.putBack, null);
      return;
    }
    if (draft.kind === "close") {
      const written = await write("close", { ...guard, outcome: draft.outcome.trim() }, draft);
      if (!written) return;
      setClosedOpen(true);
      if (written.entryId) setScrollTo(`id:${written.entryId}`);
      setNotice(tr("context.backlogClosed", "Entry closed and moved to Closed."));
      return;
    }
    const written = await write(
      "promote",
      {
        ...guard,
        task: { title: draft.title.trim(), status: "planned", client_request_id: draft.requestId },
      },
      draft,
    );
    if (!written?.taskId) return;
    setNotice(
      written.reused
        ? tr("context.backlogPromoteReused", "Opened the existing {{id}}.", { id: written.taskId })
        : tr("context.backlogPromoted", "Created {{id}} from this entry.", { id: written.taskId }),
    );
    await onTaskPromoted(written.taskId);
  }, [finishPutBack, form, onTaskPromoted, tr, write]);

  const move = useCallback(
    async (section: string) => {
      if (!form || form.kind !== "move") return;
      const written = await write(
        "move",
        { entry: formSelector(form), if_file_sha256: form.sha, section },
        form,
      );
      if (!written) return;
      setNotice(
        tr("context.backlogMoved", "Moved to {{section}}.", { section: sectionLabel(section, tr) }),
      );
      if (written.entryId) setScrollTo(`id:${written.entryId}`);
    },
    [form, tr, write],
  );

  const configure = useCallback(
    async (operation: "create" | "use", path: string, keepLocal?: boolean) => {
      setBusy(true);
      setFailure("");
      setNotice("");
      const response = await run(operation, {
        path,
        ...(operation === "create" ? { keep_local: Boolean(keepLocal) } : {}),
      });
      setBusy(false);
      if (!response.ok) {
        setFailure(
          operation === "create" && response.error.code === "backlog_file_conflict"
            ? tr(
                "context.backlogErrorExists",
                "That file already exists. Choose Use an existing file instead.",
              )
            : backlogErrorText(response.error, tr),
        );
        return;
      }
      setSwitching(false);
      setDetailsOpen(false);
    },
    [run, tr],
  );

  const excludeFile = useCallback(async () => {
    if (!snapshot) return;
    setBusy(true);
    setFailure("");
    const response = await run("exclude", { path: snapshot.path });
    setBusy(false);
    if (!response.ok) setFailure(backlogErrorText(response.error, tr));
  }, [run, snapshot, tr]);

  const archive = useCallback(
    async (task: Task) => {
      setBusy(true);
      setArchiveAsk(null);
      const error = await onArchiveTask(task);
      setBusy(false);
      if (error) setFailure(error);
      else setNotice(tr("context.backlogArchived", "Archived {{id}}.", { id: task.id }));
    },
    [onArchiveTask, tr],
  );

  if (scopeState === "loading" || (scopeState === "ready" && !snapshot && !loadError)) {
    return (
      <p className={classNames("text-sm", ui.mutedTextClass)}>{tr("common:loading", "Loading…")}</p>
    );
  }
  if (scopeState === "none") {
    return (
      <div
        className={classNames(
          "rounded-xl border border-dashed px-3 py-4 text-sm",
          "border-[var(--glass-border-subtle)] text-[var(--color-text-muted)]",
        )}
      >
        {tr(
          "context.backlogNoScope",
          "This group has no project folder yet, so it has no backlog. Attach a project folder to use one.",
        )}
      </div>
    );
  }
  if (!snapshot) {
    return (
      <div className="space-y-3">
        <p role="alert" className="text-sm text-rose-600 dark:text-rose-400">
          {backlogErrorText(loadError ?? undefined, tr)}
        </p>
        <button
          type="button"
          className={ui.buttonSecondaryClass}
          disabled={loading}
          onClick={() => void reload()}
        >
          {tr("context.backlogReload", "Reload")}
        </button>
      </div>
    );
  }

  if (!snapshot.configured || !snapshot.exists || switching) {
    return (
      <BacklogSetup
        snapshot={snapshot}
        busy={busy}
        error={failure}
        tr={tr}
        ui={ui}
        onCreate={(path, keepLocal) => void configure("create", path, keepLocal)}
        onUse={(path) => void configure("use", path)}
        onCancel={switching ? () => setSwitching(false) : undefined}
      />
    );
  }

  const counts = problemCounts(snapshot);
  const writable = !counts.fence;
  const entryForm = form && form.kind !== "add" ? form : null;
  const formEntry = entryForm ? formTarget(snapshot, entryForm) : null;
  const formKey = formEntry ? entryKey(formEntry.entry) : "";
  const addForm = form?.kind === "add" && sameFile(snapshot, form) ? form : null;
  const orphan = (entryForm && !formEntry) || (form?.kind === "add" && !addForm) ? form : null;
  const orphanText =
    !orphan || orphan.kind === "move"
      ? ""
      : orphan.kind === "add" || orphan.kind === "edit"
        ? `${orphan.title}\n\n${orphan.body}`.trim()
        : orphan.kind === "close"
          ? orphan.outcome
          : orphan.title;
  const targets = addTargets(snapshot);
  const sections = snapshot.sections.filter(
    (section) => section.name || section.entries.length > 0,
  );
  const entryTotal = snapshot.sections.reduce((sum, section) => sum + section.entries.length, 0);
  const git = snapshot.git;
  const gitText = !git
    ? tr("context.backlogGitUnknown", "Its Git status could not be read.")
    : !git.repository
      ? tr("context.backlogGitNone", "The project is not a Git repository.")
      : git.tracked
        ? tr("context.backlogGitTracked", "Git tracks this file.")
        : git.ignored
          ? tr("context.backlogGitIgnored", "Git ignores this file.")
          : tr("context.backlogGitUntracked", "Git does not track this file yet.");

  const startForm = (located: Located, kind: "edit" | "close" | "promote" | "move") => {
    const ref = entryRef(located, snapshot);
    const { entry } = located;
    const next: EntryForm =
      kind === "edit"
        ? {
            ...ref,
            kind,
            title: entry.title,
            body: entry.body,
            baseTitle: entry.title,
            baseBody: entry.body,
            putBack: null,
          }
        : kind === "close"
          ? { ...ref, kind, outcome: "" }
          : kind === "promote"
            ? { ...ref, kind, title: entry.title, requestId: newClientId() }
            : { ...ref, kind };
    replaceForm(next);
  };

  const renderSection = (section: (typeof sections)[number], index: number) => {
    const open = !section.closed || closedOpen;
    const headingId = `${domPrefix}-section-${index}`;
    const heading = (
      <>
        <span className="min-w-0 break-words">{sectionLabel(section.name, tr)}</span>
        <span
          data-backlog-section-count=""
          className={classNames("shrink-0 font-normal tabular-nums", ui.mutedTextClass)}
        >
          {section.entries.length}
        </span>
      </>
    );
    // Framed as a task column is: the head reads "Entries 13", then one record per entry.
    return (
      <section
        key={`${section.line}:${section.name}`}
        aria-labelledby={headingId}
        data-backlog-section=""
        className="min-w-0 rounded-2xl border glass-panel"
      >
        {section.closed ? (
          <button
            type="button"
            id={headingId}
            aria-expanded={open}
            onClick={() => setClosedOpen(!closedOpen)}
            data-backlog-section-head=""
            className="flex min-h-9 w-full items-center gap-2 px-3 text-left text-sm font-semibold hover:bg-[var(--color-bg-hover)] focus-visible:outline-2 focus-visible:outline-[var(--color-border-focus)]"
          >
            {open ? (
              <ChevronDownIcon size={14} aria-hidden="true" />
            ) : (
              <ChevronRightIcon size={14} aria-hidden="true" />
            )}
            {heading}
          </button>
        ) : (
          <h3
            id={headingId}
            data-backlog-section-head=""
            className="flex min-h-9 items-center gap-2 px-3 text-sm font-semibold"
          >
            {heading}
          </h3>
        )}
        {open && section.entries.length > 0 ? (
          <ul data-backlog-section-items="">
            {section.entries.map((entry) => {
              const key = entryKey(entry);
              const located = { entry, section };
              const ownForm = key === formKey ? entryForm : null;
              const returnedTask =
                returning && entry.id === returning.entryId ? returning.task : null;
              return (
                <BacklogEntryItem
                  key={key}
                  domId={entryDomId(domPrefix, key)}
                  entry={entry}
                  closed={section.closed}
                  expanded={expanded.has(key) || !!ownForm || !!returnedTask}
                  form={ownForm}
                  moveTargets={moveTargets(snapshot, section.name)}
                  writable={writable}
                  busy={busy}
                  returning={returnedTask}
                  isDark={isDark}
                  tr={tr}
                  ui={ui}
                  onToggle={() =>
                    setExpanded((current) => {
                      const next = new Set(current);
                      if (next.has(key)) next.delete(key);
                      else next.add(key);
                      return next;
                    })
                  }
                  onOpenTask={onOpenTask}
                  onStartForm={(kind) => startForm(located, kind)}
                  onFormChange={(next) => setForm(() => next)}
                  onSubmit={() => void submit()}
                  onCancel={() => {
                    setFailure("");
                    setForm(() => null);
                  }}
                  onMove={(target) => void move(target)}
                  onReturnWrite={() => {
                    if (!returnedTask) return;
                    const paragraph = taskResultParagraph(returnedTask, todayLocal());
                    if (openEdit(snapshot, located, paragraph, null)) setReturning(null);
                  }}
                  onReturnClose={() => {
                    if (!returnedTask) return;
                    const opened = replaceForm({
                      ...entryRef(located, snapshot),
                      kind: "close",
                      outcome: firstLine(returnedTask.outcome),
                    });
                    if (opened) setReturning(null);
                  }}
                  onReturnDismiss={() => setReturning(null)}
                />
              );
            })}
          </ul>
        ) : null}
      </section>
    );
  };

  return (
    <div className="space-y-4" data-backlog-panel="">
      <div className="flex flex-wrap items-start justify-between gap-2">
        <button
          type="button"
          aria-expanded={detailsOpen}
          onClick={() => setDetailsOpen(!detailsOpen)}
          title={tr("context.backlogFileDetails", "File details")}
          className="flex min-h-9 min-w-0 items-center gap-1.5 rounded-md px-1 text-left text-sm font-medium hover:bg-[var(--color-bg-hover)] focus-visible:outline-2 focus-visible:outline-[var(--color-border-focus)]"
        >
          {detailsOpen ? (
            <ChevronDownIcon size={14} className="shrink-0" aria-hidden="true" />
          ) : (
            <ChevronRightIcon size={14} className="shrink-0" aria-hidden="true" />
          )}
          <span className="min-w-0 break-all">{snapshot.path}</span>
        </button>
        <div className="flex shrink-0 items-center gap-2">
          <button
            type="button"
            className={ui.buttonSecondaryClass}
            disabled={loading || busy}
            aria-label={tr("context.backlogReload", "Reload")}
            title={tr("context.backlogReload", "Reload")}
            onClick={() => void reload()}
          >
            <RefreshIcon size={16} aria-hidden="true" />
          </button>
          {writable ? (
            <button
              type="button"
              className={classNames(ui.buttonSecondaryClass, "gap-1.5")}
              disabled={busy}
              onClick={() =>
                replaceForm({
                  ...fileRef(snapshot),
                  kind: "add",
                  section: defaultSection(snapshot),
                  title: "",
                  body: "",
                  putBack: null,
                })
              }
            >
              <PlusIcon size={16} aria-hidden="true" />
              {tr("context.backlogAdd", "Entry")}
            </button>
          ) : null}
        </div>
      </div>

      {detailsOpen ? (
        <div className="space-y-2 rounded-lg border border-[var(--glass-border-subtle)] px-3 py-2 text-sm">
          <p className={ui.subtleTextClass}>{gitText}</p>
          <div className="flex flex-wrap gap-2">
            {git?.repository && !git.tracked && !git.ignored ? (
              <button
                type="button"
                className={ui.buttonSecondaryClass}
                disabled={busy}
                onClick={() => void excludeFile()}
              >
                {tr("context.backlogKeepLocal", "Keep it on this computer")}
              </button>
            ) : null}
            <button
              type="button"
              className={ui.buttonSecondaryClass}
              disabled={busy}
              onClick={() => {
                setFailure("");
                setSwitching(true);
              }}
            >
              {tr("context.backlogSwitchFile", "Use another file…")}
            </button>
          </div>
        </div>
      ) : null}

      {counts.fence || counts.blocked || counts.duplicateSection || counts.missingId ? (
        <div className="space-y-1 text-xs">
          {counts.fence ? (
            <p className="text-amber-700 dark:text-amber-400">
              {tr(
                "context.backlogProblemFence",
                "The file has a code block (```) that is never closed. Nothing can be changed here until it is closed in the file.",
              )}
            </p>
          ) : null}
          {counts.blocked ? (
            <p className="text-amber-700 dark:text-amber-400">
              {tr(
                "context.backlogProblemBlocked",
                "{{count}} entries need their heading fixed in the file before they can be changed here.",
                { count: counts.blocked },
              )}
            </p>
          ) : null}
          {counts.duplicateSection ? (
            <p className="text-amber-700 dark:text-amber-400">
              {tr(
                "context.backlogProblemDuplicateSection",
                "Two sections have the same name. Entries cannot be added to or moved into them until one is renamed in the file.",
              )}
            </p>
          ) : null}
          {counts.missingId ? (
            <p className={ui.mutedTextClass}>
              {tr(
                "context.backlogProblemMissingId",
                "{{count}} entries have no ID yet; each gets one the first time it is changed here.",
                { count: counts.missingId },
              )}
            </p>
          ) : null}
        </div>
      ) : null}

      {failure ? (
        <div
          role="alert"
          className="space-y-2 rounded-lg border border-rose-500/30 bg-rose-500/15 px-3 py-2 text-sm text-rose-600 dark:text-rose-400"
        >
          <p className="break-words">{failure}</p>
          <button
            type="button"
            className={ui.buttonSecondaryClass}
            disabled={busy || loading}
            onClick={() => {
              setFailure("");
              void reload();
            }}
          >
            {tr("context.backlogReload", "Reload")}
          </button>
        </div>
      ) : null}
      {linkFailed ? (
        <div
          role="alert"
          className="space-y-2 rounded-lg border border-rose-500/30 bg-rose-500/15 px-3 py-2 text-sm text-rose-600 dark:text-rose-400"
        >
          <p className="break-words">
            {tr(
              "context.backlogLinkFailed",
              "The entry was saved, but {{id}} could not be linked to it: {{error}}",
              { id: linkFailed.task.id, error: linkFailed.error },
            )}
          </p>
          <div className="flex flex-wrap gap-2">
            <button
              type="button"
              className={ui.buttonSecondaryClass}
              disabled={busy}
              onClick={() => void finishPutBack(linkFailed.task, linkFailed.source)}
            >
              {tr("common:retry", "Retry")}
            </button>
            <button
              type="button"
              className={ui.buttonSecondaryClass}
              disabled={busy}
              onClick={() => setLinkFailed(null)}
            >
              {tr("context.backlogDismiss", "Not now")}
            </button>
          </div>
        </div>
      ) : null}
      {notice ? (
        <p role="status" className="text-sm text-[var(--color-text-secondary)]">
          {notice}
        </p>
      ) : null}
      {archiveAsk ? (
        <div className="space-y-2 rounded-lg border border-[var(--glass-border-subtle)] px-3 py-2 text-sm">
          <p>
            {tr(
              "context.backlogArchiveAsk",
              "{{id}} is back in the backlog. Archive the task too?",
              { id: archiveAsk.id },
            )}
          </p>
          <div className="flex flex-wrap gap-2">
            <button
              type="button"
              className={ui.buttonSecondaryClass}
              disabled={busy}
              onClick={() => void archive(archiveAsk)}
            >
              {tr("context.backlogArchiveConfirm", "Archive {{id}}", { id: archiveAsk.id })}
            </button>
            <button
              type="button"
              className={ui.buttonSecondaryClass}
              disabled={busy}
              onClick={() => setArchiveAsk(null)}
            >
              {tr("context.backlogDismiss", "Not now")}
            </button>
          </div>
        </div>
      ) : null}

      {orphan ? (
        <div className="space-y-2 rounded-lg border border-amber-500/30 bg-amber-500/10 px-3 py-2 text-sm">
          <p className="text-amber-700 dark:text-amber-300">
            {tr(
              "context.backlogDraftOrphaned",
              "The file or the entry this draft was for has changed, so the draft cannot be saved. Copy what you want to keep, then start again from the current text.",
            )}
          </p>
          {orphanText ? (
            <textarea
              readOnly
              aria-label={tr("context.backlogDraftText", "Your draft")}
              className={classNames(ui.textareaClass, "font-mono text-xs")}
              value={orphanText}
            />
          ) : null}
          <button
            type="button"
            className={ui.buttonSecondaryClass}
            onClick={() => setForm(() => null)}
          >
            {tr("context.backlogDiscard", "Discard draft")}
          </button>
        </div>
      ) : null}

      {addForm ? (
        <form
          className="space-y-3 rounded-lg border border-[var(--glass-border-subtle)] px-3 py-3"
          onSubmit={(event) => {
            event.preventDefault();
            void submit();
          }}
        >
          {addForm.putBack ? (
            <p className={classNames("text-sm", ui.subtleTextClass)}>
              {tr(
                "context.backlogPutBackNew",
                "{{id}} goes back as a new entry, and the task is linked to it.",
                { id: addForm.putBack.id },
              )}
            </p>
          ) : null}
          {targets.length > 1 || !targets.includes(addForm.section) ? (
            <SelectCombobox
              items={targets.map((name) => ({ value: `=${name}`, label: sectionLabel(name, tr) }))}
              value={`=${addForm.section}`}
              onChange={(value) => setForm(() => ({ ...addForm, section: value.slice(1) }))}
              ariaLabel={tr("context.backlogSection", "Section")}
              className={ui.inputClass}
              disabled={busy}
            />
          ) : null}
          <div className="space-y-1">
            <label htmlFor={addTitleId} className="block text-xs font-medium">
              {tr("context.titleField", "Title")}
            </label>
            <input
              id={addTitleId}
              className={ui.inputClass}
              value={addForm.title}
              readOnly={busy}
              autoFocus
              onChange={(event) => setForm(() => ({ ...addForm, title: event.target.value }))}
            />
          </div>
          <div className="space-y-1">
            <label htmlFor={addBodyId} className="block text-xs font-medium">
              {tr("context.backlogBodyLabel", "Text")}
            </label>
            <textarea
              id={addBodyId}
              className={classNames(ui.textareaClass, "min-h-[120px] font-mono text-xs")}
              value={addForm.body}
              readOnly={busy}
              placeholder={tr(
                "context.backlogBodyPlaceholder",
                "The problem, why it waits, and when to look at it again. Use #### for headings.",
              )}
              onChange={(event) => setForm(() => ({ ...addForm, body: event.target.value }))}
            />
          </div>
          <div className="flex flex-wrap gap-2">
            <button
              type="submit"
              className={ui.buttonPrimaryClass}
              disabled={busy || !addForm.title.trim()}
            >
              {tr("context.backlogAddConfirm", "Add entry")}
            </button>
            <button
              type="button"
              className={ui.buttonSecondaryClass}
              disabled={busy}
              onClick={() => {
                setFailure("");
                setForm(() => null);
              }}
            >
              {tr("context.cancel", "Cancel")}
            </button>
          </div>
        </form>
      ) : null}

      {entryTotal === 0 ? (
        <p className={classNames("text-sm", ui.mutedTextClass)}>
          {tr("context.backlogEmpty", "No entries yet.")}
        </p>
      ) : null}
      <div className="space-y-3">{sections.map(renderSection)}</div>
    </div>
  );
}
