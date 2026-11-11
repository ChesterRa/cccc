import type {
  BacklogEntry,
  BacklogSection,
  BacklogSnapshot,
  BacklogSource,
  Task,
} from "../../../types";
import type { BacklogEntrySelector } from "../../../services/api";
import type { ContextTranslator } from "../model";

/**
 * What the task side asks of the backlog view: show the entry a task came from, take a task's
 * result back to it, or put a task back into the backlog.
 */
export type BacklogIntent =
  | { kind: "show"; source: BacklogSource }
  | { kind: "return"; task: Task }
  | { kind: "putBack"; task: Task };

/**
 * The file a draft was made for, in the folder it was read from. A draft never follows the
 * backlog to another file, or to another folder attached under the same project key.
 */
type FileRef = { scopeKey: string; scopeUrl: string; path: string };

/**
 * What an entry form writes to: its file, the version of it the form was checked against, which
 * a write sends as its guard, and the entry as that version had it.
 */
export type EntryRef = FileRef & {
  entryId: string | null;
  line: number;
  sha: string;
  seen: string;
};

export type BacklogForm =
  | (FileRef & {
      kind: "add";
      section: string;
      title: string;
      body: string;
      /** The task being put back, which the new entry becomes the source of. */
      putBack: Task | null;
    })
  | (EntryRef & {
      kind: "edit";
      title: string;
      body: string;
      /** The entry as the draft started from it, to tell whether anything was typed. */
      baseTitle: string;
      baseBody: string;
      putBack: Task | null;
    })
  | (EntryRef & { kind: "close"; outcome: string })
  | (EntryRef & { kind: "promote"; title: string; requestId: string })
  | (EntryRef & { kind: "move" });

export type EntryForm = Exclude<BacklogForm, { kind: "add" }>;

/** A draft that would be lost by closing it or opening another one. */
export function backlogFormDirty(form: BacklogForm | null): boolean {
  if (!form) return false;
  switch (form.kind) {
    case "add":
      return Boolean(form.title.trim() || form.body.trim());
    case "edit":
      return form.title !== form.baseTitle || form.body !== form.baseBody;
    case "close":
      return Boolean(form.outcome.trim());
    default:
      return false;
  }
}

type Located = { entry: BacklogEntry; section: BacklogSection };

/** The parts of an entry a write depends on: where it is, and what it says. */
export function seenOf({ entry, section }: Located): string {
  return JSON.stringify([section.name, entry.title, entry.body]);
}

export function formSelector(form: EntryRef): BacklogEntrySelector {
  return form.entryId ? { id: form.entryId } : { line: form.line };
}

export function fileRef(snapshot: BacklogSnapshot): FileRef {
  return { scopeKey: snapshot.scope_key, scopeUrl: snapshot.scope_url, path: snapshot.path };
}

export function sameFile(snapshot: BacklogSnapshot, form: FileRef): boolean {
  return (
    snapshot.scope_key === form.scopeKey &&
    snapshot.scope_url === form.scopeUrl &&
    snapshot.path === form.path
  );
}

export function sameSource(a: BacklogSource | null | undefined, b: BacklogSource): boolean {
  return !!a && a.scope_key === b.scope_key && a.path === b.path && a.entry_id === b.entry_id;
}

/** Whether a task's source names the file this snapshot shows. */
export function sourceInFile(
  snapshot: BacklogSnapshot,
  source: BacklogSource | null | undefined,
): source is BacklogSource {
  return !!source && source.scope_key === snapshot.scope_key && source.path === snapshot.path;
}

export function entryRef(located: Located, snapshot: BacklogSnapshot): EntryRef {
  return {
    ...fileRef(snapshot),
    entryId: located.entry.id,
    line: located.entry.line,
    sha: snapshot.sha256,
    seen: seenOf(located),
  };
}

/**
 * The entry a form belongs to in this file version: in the same file, found by its ID or, until
 * it has one, by its heading line, and still where and as the form saw it. Anything else leaves
 * the draft without an entry, to be copied rather than written over a change it never saw.
 */
export function formTarget(snapshot: BacklogSnapshot | null, form: EntryRef): Located | null {
  if (!snapshot || !sameFile(snapshot, form)) return null;
  const located = form.entryId
    ? findEntryById(snapshot, form.entryId)
    : findEntryWhere(snapshot, (entry) => !entry.id && entry.line === form.line);
  return located && seenOf(located) === form.seen ? located : null;
}

/** The first line of a task's outcome, which is what a one-line field can take. */
export function firstLine(text: string | null | undefined): string {
  return (
    String(text || "")
      .split(/\r?\n/)
      .map((line) => line.trim())
      .find(Boolean) || ""
  );
}

/**
 * Stable within one file version. An entry without an ID, or with one another entry repeats, is
 * known by its heading line.
 */
export function entryKey(entry: BacklogEntry): string {
  const unique = entry.id && !entry.problems.some((problem) => problem.code === "duplicate_id");
  return unique ? `id:${entry.id}` : `line:${entry.line}`;
}

/** The one entry that matches; none when no entry or more than one does. */
function findEntryWhere(
  snapshot: BacklogSnapshot | null,
  matches: (entry: BacklogEntry) => boolean,
): Located | null {
  let found: Located | null = null;
  for (const section of snapshot?.sections || []) {
    for (const entry of section.entries) {
      if (!matches(entry)) continue;
      if (found) return null;
      found = { entry, section };
    }
  }
  return found;
}

/** The entry with this ID; none when the file repeats it, as the daemon then refuses too. */
export function findEntryById(snapshot: BacklogSnapshot | null, id: string): Located | null {
  return id ? findEntryWhere(snapshot, (entry) => entry.id === id) : null;
}

/**
 * Sections an entry can be added or moved to, in file order. Closed is reached only by closing
 * an entry, with an outcome; a name used twice cannot be told apart, so it is left out. The part
 * before the first section counts when it holds entries or there is nowhere else.
 */
export function addTargets(snapshot: BacklogSnapshot): string[] {
  const names = snapshot.sections
    .filter((section) => section.name && !section.closed)
    .map((section) => section.name);
  const unique = names.filter((name) => names.indexOf(name) === names.lastIndexOf(name));
  const preamble = snapshot.sections.find((section) => !section.name);
  return preamble?.entries.length || unique.length === 0 ? ["", ...unique] : unique;
}

/** Where a new entry goes unless the person picks a section: the first one that is not Closed. */
export function defaultSection(snapshot: BacklogSnapshot): string {
  return addTargets(snapshot).find(Boolean) ?? "";
}

export function moveTargets(snapshot: BacklogSnapshot, current: string): string[] {
  return addTargets(snapshot).filter((name) => name !== current);
}

export function sectionLabel(name: string, tr: ContextTranslator): string {
  return name || tr("context.backlogPreamble", "Before the first section");
}

/** Adds a paragraph after the body's text, keeping one blank line between them. */
export function appendParagraph(body: string, paragraph: string): string {
  const text = body.replace(/\s+$/, "");
  return text ? `${text}\n\n${paragraph}\n` : `${paragraph}\n`;
}

/**
 * A task's result as a paragraph for its entry, dated like a close; empty when it has none. The
 * task's ID stays out: it names a task in this CCCC only, and the file may travel further.
 */
export function taskResultParagraph(task: Task, date: string): string {
  const outcome = String(task.outcome || "").trim();
  return outcome ? `Outcome (${date}): ${outcome}` : "";
}

export function todayLocal(now = new Date()): string {
  const month = String(now.getMonth() + 1).padStart(2, "0");
  const day = String(now.getDate()).padStart(2, "0");
  return `${now.getFullYear()}-${month}-${day}`;
}

/** An entry that cannot be changed here until its heading is fixed in the file. */
export function entryBlocked(entry: BacklogEntry): boolean {
  return entry.problems.some((problem) => problem.code !== "missing_id");
}

export type ProblemCounts = {
  /** Entries without an ID yet; each gets one when it is first changed here. */
  missingId: number;
  /** Entries whose heading must be fixed in the file before they can be changed here. */
  blocked: number;
  /** A code block is never closed, so nothing can be written until it is. */
  fence: boolean;
  duplicateSection: boolean;
};

export function problemCounts(snapshot: BacklogSnapshot): ProblemCounts {
  const entries = snapshot.sections.flatMap((section) => section.entries);
  return {
    missingId: entries.filter((entry) =>
      entry.problems.some((problem) => problem.code === "missing_id"),
    ).length,
    blocked: entries.filter(entryBlocked).length,
    fence: snapshot.problems.some((problem) => problem.code === "unterminated_fence"),
    duplicateSection: snapshot.problems.some((problem) => problem.code === "duplicate_section"),
  };
}

type ApiError = { code: string; message: string; details?: unknown };

function record(value: unknown): Record<string, unknown> | null {
  return value && typeof value === "object" && !Array.isArray(value)
    ? (value as Record<string, unknown>)
    : null;
}

/**
 * The entry a write that failed part way had already saved to the file, by the daemon's own
 * account; empty when it did not say so.
 */
export function savedEntryId(error: ApiError | undefined): string {
  if (error?.code !== "backlog_partial_failure") return "";
  const file = record(record(record(error.details)?.applied)?.file);
  return typeof file?.entry_id === "string" ? file.entry_id : "";
}

/**
 * Says what happened in the person's language. A partial failure states what was written, and
 * that the rest is unconfirmed, never that it did not happen.
 */
export function backlogErrorText(error: ApiError | undefined, tr: ContextTranslator): string {
  const details = record(error?.details);
  const reason = typeof details?.reason === "string" ? details.reason : "";
  switch (error?.code) {
    case "backlog_file_conflict":
      return tr(
        "context.backlogErrorChanged",
        "The file changed after this page read it, so nothing was written. Your text is kept: reload, then save again.",
      );
    case "scope_conflict":
      return tr(
        "context.backlogErrorScopeChanged",
        "The project folder changed, so nothing was written. Reload first.",
      );
    case "backlog_partial_failure": {
      const applied = record(details?.applied);
      const file = record(applied?.file);
      const facts = [
        file ? tr("context.backlogAppliedFile", "the file was saved") : "",
        applied?.configured === true
          ? tr("context.backlogAppliedConfigured", "the file was chosen")
          : "",
        applied?.excluded === true
          ? tr("context.backlogAppliedExcluded", "the Git exclude rule was added")
          : "",
      ].filter(Boolean);
      if (facts.length === 0) {
        return tr(
          "context.backlogErrorUnconfirmed",
          "The result could not be confirmed. Reload to see where things stand before trying again.",
        );
      }
      return tr(
        "context.backlogErrorPartial",
        "Done so far: {{facts}}. The next step is unconfirmed; reload to see where things stand before trying again.",
        { facts: facts.join(tr("context.backlogListSeparator", ", ")) },
      );
    }
    case "backlog_not_found":
      return reason === "entry_not_found"
        ? tr("context.backlogEntryMissing", "That entry is not in the current backlog file.")
        : reason === "section_not_found"
          ? tr(
              "context.backlogErrorSectionGone",
              "That section is no longer in the file. Reload to see the current file.",
            )
          : tr(
              "context.backlogErrorFileMissing",
              "The file, or the folder it should be in, does not exist.",
            );
    case "backlog_invalid_file":
      return tr(
        "context.backlogErrorInvalidFile",
        "This file is not UTF-8 text of at most 1 MiB, so it cannot be used as the backlog.",
      );
    case "backlog_tracked":
      return tr(
        "context.backlogErrorTracked",
        "Git already tracks this file, so an exclude rule cannot keep it local.",
      );
    case "backlog_git_unavailable":
      return tr(
        "context.backlogErrorNoGit",
        "This project is not a Git repository, or Git could not be run.",
      );
    case "backlog_not_configured":
      return tr("context.backlogErrorNotConfigured", "No backlog file is chosen yet.");
    case "backlog_ambiguous":
    case "backlog_invalid_entry":
      break;
    default:
      return error?.message || tr("context.failedToApplyChanges", "Failed to apply changes");
  }
  switch (reason) {
    case "entry_not_found":
      return tr("context.backlogEntryMissing", "That entry is not in the current backlog file.");
    case "section_not_found":
      return tr(
        "context.backlogErrorSectionGone",
        "That section is no longer in the file. Reload to see the current file.",
      );
    case "ambiguous_entry":
      return tr(
        "context.backlogErrorEntryProblem",
        "This entry's heading has a damaged or repeated ID. Fix its heading in the file first.",
      );
    case "ambiguous_section":
      return tr(
        "context.backlogProblemDuplicateSection",
        "Two sections have the same name. Entries cannot be added to or moved into them until one is renamed in the file.",
      );
    case "unterminated_fence":
      return tr(
        "context.backlogProblemFence",
        "The file has a code block (```) that is never closed. Nothing can be changed here until it is closed in the file.",
      );
    case "invalid_title":
      return tr("context.backlogErrorTitle", "The title must be one line and not empty.");
    case "structural_body":
      return tr(
        "context.backlogErrorBody",
        "The text cannot contain lines starting with ## or ###, or a code block left open. Use #### for headings inside an entry.",
      );
    case "invalid_outcome":
      return tr("context.backlogErrorOutcome", "The outcome must be one line and not empty.");
    case "closed_target":
      return tr(
        "context.backlogErrorClosedTarget",
        "Entries reach Closed only by closing them with an outcome.",
      );
    case "entry_problem":
      return tr(
        "context.backlogErrorEntryProblem",
        "This entry's heading has a damaged or repeated ID. Fix its heading in the file first.",
      );
    default:
      return error?.message || tr("context.failedToApplyChanges", "Failed to apply changes");
  }
}
