import { useId, useState } from "react";
import type { BacklogSnapshot } from "../../../types";
import { classNames } from "../../../utils/classNames";
import type { ContextTranslator } from "../model";
import type { ContextModalUi } from "../ui";

type SetupForm =
  | { kind: "create"; path: string; keepLocal: boolean }
  | { kind: "use"; path: string };

/**
 * Choosing the backlog file: a new one, or one the project already has. Nothing is written
 * until the person confirms; until then tasks work as before.
 */
export function BacklogSetup({
  snapshot,
  busy,
  error,
  tr,
  ui,
  onCreate,
  onUse,
  onCancel,
}: {
  /** Null when the backlog could not be read at all. */
  snapshot: BacklogSnapshot | null;
  busy: boolean;
  error: string;
  tr: ContextTranslator;
  ui: ContextModalUi;
  onCreate: (path: string, keepLocal: boolean) => void;
  onUse: (path: string) => void;
  /** Given when a file is already in use: going back keeps it. */
  onCancel?: () => void;
}) {
  const [form, setForm] = useState<SetupForm | null>(null);
  const pathId = useId();
  const keepLocalId = useId();
  // Only a project Git can read gets the choice; elsewhere there is no exclude rule to add.
  const gitRepository = snapshot?.git?.repository === true;
  const missingPath = snapshot?.configured && !snapshot.exists ? snapshot.path : "";

  const submit = () => {
    if (!form) return;
    const path = form.path.trim();
    if (!path) return;
    if (form.kind === "create") onCreate(path, gitRepository && form.keepLocal);
    else onUse(path);
  };

  return (
    <div className="space-y-3 text-sm">
      {missingPath ? (
        <p className="text-[var(--color-text-primary)]">
          {tr("context.backlogMissingFile", "{{path}} is not there any more.", {
            path: missingPath,
          })}
        </p>
      ) : (
        <p className={ui.subtleTextClass}>
          {tr(
            "context.backlogIntro",
            "The backlog is a Markdown file in the project for long-term topics and ideas. Tasks made from an entry track the work on it.",
          )}
        </p>
      )}
      {form ? (
        <form
          className="space-y-3"
          onSubmit={(event) => {
            event.preventDefault();
            submit();
          }}
        >
          <div className="space-y-1">
            <label htmlFor={pathId} className="block text-xs font-medium">
              {tr("context.backlogPathLabel", "Path in the project")}
            </label>
            <input
              id={pathId}
              className={ui.inputClass}
              value={form.path}
              autoFocus
              spellCheck={false}
              placeholder={form.kind === "create" ? "BACKLOG.md" : "docs/BACKLOG.md"}
              onChange={(event) => setForm({ ...form, path: event.target.value })}
            />
          </div>
          {form.kind === "create" && gitRepository ? (
            <div className="flex items-start gap-2">
              <input
                id={keepLocalId}
                type="checkbox"
                className="mt-1"
                checked={form.keepLocal}
                onChange={(event) => setForm({ ...form, keepLocal: event.target.checked })}
              />
              <label htmlFor={keepLocalId} className="space-y-0.5">
                <span className="block">
                  {tr("context.backlogKeepLocal", "Keep it on this computer")}
                </span>
                <span className={classNames("block text-xs", ui.mutedTextClass)}>
                  {tr(
                    "context.backlogKeepLocalOptionHint",
                    "Adds it to this clone's Git exclude list, so git add -A leaves it out. .gitignore is not changed.",
                  )}
                </span>
              </label>
            </div>
          ) : null}
          {error ? (
            <p role="alert" className="text-rose-600 dark:text-rose-400">
              {error}
            </p>
          ) : null}
          <div className="flex flex-wrap gap-2">
            <button
              type="submit"
              className={ui.buttonPrimaryClass}
              disabled={busy || !form.path.trim()}
            >
              {form.kind === "create"
                ? tr("context.createButton", "Create")
                : tr("context.backlogUseConfirm", "Use this file")}
            </button>
            <button
              type="button"
              className={ui.buttonSecondaryClass}
              disabled={busy}
              onClick={() => setForm(null)}
            >
              {tr("context.cancel", "Cancel")}
            </button>
          </div>
        </form>
      ) : (
        <>
          {error ? (
            <p role="alert" className="text-rose-600 dark:text-rose-400">
              {error}
            </p>
          ) : null}
          <div className="flex flex-wrap gap-2">
            <button
              type="button"
              className={ui.buttonPrimaryClass}
              disabled={busy}
              onClick={() => setForm({ kind: "create", path: "BACKLOG.md", keepLocal: true })}
            >
              {tr("context.backlogCreate", "Create BACKLOG.md…")}
            </button>
            <button
              type="button"
              className={ui.buttonSecondaryClass}
              disabled={busy}
              onClick={() => setForm({ kind: "use", path: "" })}
            >
              {tr("context.backlogUseExisting", "Use an existing file…")}
            </button>
            {onCancel ? (
              <button
                type="button"
                className={ui.buttonSecondaryClass}
                disabled={busy}
                onClick={onCancel}
              >
                {tr("context.cancel", "Cancel")}
              </button>
            ) : null}
          </div>
          {onCancel ? null : (
            <p className={classNames("text-xs", ui.mutedTextClass)}>
              {tr("context.backlogNotNow", "You can also skip this: tasks work without a backlog.")}
            </p>
          )}
        </>
      )}
    </div>
  );
}
