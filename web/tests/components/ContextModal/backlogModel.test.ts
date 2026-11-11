import { describe, expect, it } from "vite-plus/test";
import type { BacklogEntry, BacklogSection, BacklogSnapshot, Task } from "../../../src/types";
import {
  addTargets,
  appendParagraph,
  backlogErrorText,
  backlogFormDirty,
  entryKey,
  entryRef,
  findEntryById,
  formTarget,
  problemCounts,
  sameSource,
  savedEntryId,
  sourceInFile,
  taskResultParagraph,
  type BacklogForm,
} from "../../../src/components/ContextModal/backlog/backlogModel";

const tr = (_key: string, fallback: string, options?: Record<string, unknown>) =>
  fallback.replace(/\{\{(\w+)\}\}/g, (_match, name: string) => String(options?.[name] ?? ""));

function entry(id: string | null, title: string, line: number, body = ""): BacklogEntry {
  return { id, title, line, body, problems: [], tasks: [] };
}

function section(name: string, line: number, entries: BacklogEntry[], closed = false) {
  return { name, line, closed, entries } satisfies BacklogSection;
}

function snapshot(sections: BacklogSection[], overrides: Partial<BacklogSnapshot> = {}) {
  return {
    configured: true,
    scope_key: "s_one",
    scope_url: "file:///repo",
    path: "BACKLOG.md",
    exists: true,
    sha256: "a".repeat(64),
    git: { repository: true, tracked: false, ignored: true },
    problems: [],
    sections,
    ...overrides,
  } satisfies BacklogSnapshot;
}

const base = snapshot([
  section("Now", 3, [entry("aaaaaaaaaaaa", "Sync speed", 5, "Slow on big groups.\n")]),
  section("Later", 9, [entry(null, "Dark mode", 11, "Needs a palette.\n")]),
]);

function editForm(snap: BacklogSnapshot, id: string): BacklogForm & { kind: "edit" } {
  const located = findEntryById(snap, id)!;
  return {
    ...entryRef(located, snap),
    kind: "edit",
    title: located.entry.title,
    body: located.entry.body,
    baseTitle: located.entry.title,
    baseBody: located.entry.body,
    putBack: null,
  };
}

describe("formTarget", () => {
  it("finds the entry in the same file while it is unchanged", () => {
    const form = editForm(base, "aaaaaaaaaaaa");
    const next = snapshot(
      [
        section("Now", 3, [entry("aaaaaaaaaaaa", "Sync speed", 5, "Slow on big groups.\n")]),
        section("Later", 9, [entry(null, "Dark mode", 11, "Needs a palette, and contrast.\n")]),
      ],
      { sha256: "b".repeat(64) },
    );
    expect(formTarget(next, form)?.entry.id).toBe("aaaaaaaaaaaa");
  });

  it("lets go of an entry whose text, title or section changed", () => {
    const form = editForm(base, "aaaaaaaaaaaa");
    const changedBody = snapshot([
      section("Now", 3, [entry("aaaaaaaaaaaa", "Sync speed", 5, "Fixed in part.\n")]),
    ]);
    const changedTitle = snapshot([
      section("Now", 3, [entry("aaaaaaaaaaaa", "Sync", 5, "Slow on big groups.\n")]),
    ]);
    const moved = snapshot([
      section("Later", 3, [entry("aaaaaaaaaaaa", "Sync speed", 5, "Slow on big groups.\n")]),
    ]);
    expect(formTarget(changedBody, form)).toBeNull();
    expect(formTarget(changedTitle, form)).toBeNull();
    expect(formTarget(moved, form)).toBeNull();
  });

  it("never follows a draft to another file or project folder, even with the same text", () => {
    const form = editForm(base, "aaaaaaaaaaaa");
    expect(formTarget({ ...base, path: "docs/BACKLOG.md" }, form)).toBeNull();
    expect(formTarget({ ...base, scope_key: "s_two" }, form)).toBeNull();
    // Another clone of the same repository shares the project key but not the folder.
    expect(formTarget({ ...base, scope_url: "file:///clone" }, form)).toBeNull();
  });

  it("finds an entry without an ID by its heading line only", () => {
    const located = base.sections[1].entries[0];
    const form = { ...entryRef({ entry: located, section: base.sections[1] }, base) };
    expect(form.entryId).toBeNull();
    expect(formTarget(base, form)?.entry.title).toBe("Dark mode");
    const shifted = snapshot([
      section("Later", 9, [entry(null, "Dark mode", 13, "Needs a palette.\n")]),
    ]);
    expect(formTarget(shifted, form)).toBeNull();
  });
});

describe("entries that repeat an ID", () => {
  // Copying a topic in an editor copies its hidden ID too.
  const duplicate = (line: number) => ({
    ...entry("cccccccccccc", "Copied topic", line, "Same text.\n"),
    problems: [{ code: "duplicate_id", line, message: "" }],
  });
  const snap = snapshot([section("Now", 1, [duplicate(3), duplicate(7)])]);

  it("are found by neither copy's ID and keep apart by heading line", () => {
    expect(findEntryById(snap, "cccccccccccc")).toBeNull();
    expect(snap.sections[0].entries.map(entryKey)).toEqual(["line:3", "line:7"]);
  });

  it("do not take a draft made for an entry without an ID at the same line", () => {
    const idless = snapshot([section("Now", 1, [entry(null, "Copied topic", 3, "Same text.\n")])]);
    const [now] = idless.sections;
    const form = entryRef({ entry: now.entries[0], section: now }, idless);
    expect(formTarget(idless, form)?.entry.line).toBe(3);
    expect(formTarget(snap, form)).toBeNull();
  });
});

describe("sourceInFile", () => {
  it("matches a task's source only in the file it names", () => {
    const source = { scope_key: "s_one", path: "BACKLOG.md", entry_id: "aaaaaaaaaaaa" };
    expect(sourceInFile(base, source)).toBe(true);
    expect(sourceInFile(base, { ...source, path: "OTHER.md" })).toBe(false);
    expect(sourceInFile(base, { ...source, scope_key: "s_two" })).toBe(false);
    expect(sourceInFile(base, null)).toBe(false);
  });

  it("tells a task's own entry from another", () => {
    const source = { scope_key: "s_one", path: "BACKLOG.md", entry_id: "aaaaaaaaaaaa" };
    expect(sameSource({ ...source }, source)).toBe(true);
    expect(sameSource({ ...source, entry_id: "bbbbbbbbbbbb" }, source)).toBe(false);
    expect(sameSource(null, source)).toBe(false);
  });
});

describe("addTargets", () => {
  it("leaves out Closed and names used twice, in file order", () => {
    const snap = snapshot([
      section("", 1, []),
      section("Now", 3, []),
      section("Ideas", 5, []),
      section("Ideas", 7, []),
      section("Closed", 9, [], true),
    ]);
    expect(addTargets(snap)).toEqual(["Now"]);
  });

  it("offers the text before the first section when it has entries or nothing else exists", () => {
    expect(addTargets(snapshot([section("", 1, [])]))).toEqual([""]);
    expect(
      addTargets(snapshot([section("", 1, [entry(null, "Loose", 2)]), section("Now", 4, [])])),
    ).toEqual(["", "Now"]);
    expect(addTargets(snapshot([section("", 1, []), section("Now", 4, [])]))).toEqual(["Now"]);
  });
});

describe("task results in the file", () => {
  const task: Task = { id: "T42", title: "Cache tasks", outcome: "Halved the load time." };

  it("writes the outcome and its date, never the task's ID", () => {
    const paragraph = taskResultParagraph(task, "2026-11-11");
    expect(paragraph).toBe("Outcome (2026-11-11): Halved the load time.");
    expect(paragraph).not.toContain("T42");
    expect(taskResultParagraph({ ...task, outcome: "  " }, "2026-11-11")).toBe("");
  });

  it("adds a paragraph after the text with one blank line between", () => {
    expect(appendParagraph("First.\n\n\n", "Next.")).toBe("First.\n\nNext.\n");
    expect(appendParagraph("", "Next.")).toBe("Next.\n");
  });
});

describe("drafts", () => {
  it("counts only typed text as unsaved", () => {
    const form = editForm(base, "aaaaaaaaaaaa");
    expect(backlogFormDirty(form)).toBe(false);
    expect(backlogFormDirty({ ...form, body: "Changed.\n" })).toBe(true);
    expect(
      backlogFormDirty({ ...entryRef(findEntryById(base, "aaaaaaaaaaaa")!, base), kind: "move" }),
    ).toBe(false);
    expect(
      backlogFormDirty({
        scopeKey: "s_one",
        scopeUrl: "file:///repo",
        path: "BACKLOG.md",
        kind: "add",
        section: "Now",
        title: " ",
        body: "",
        putBack: null,
      }),
    ).toBe(false);
  });
});

describe("problemCounts", () => {
  it("keeps entries without an ID writable and counts damaged headings as blocked", () => {
    const missing = {
      ...entry(null, "No ID", 3),
      problems: [{ code: "missing_id", line: 3, message: "" }],
    };
    const damaged = {
      ...entry(null, "Bad ID", 5),
      problems: [{ code: "malformed_id", line: 5, message: "" }],
    };
    const counts = problemCounts(
      snapshot([section("Now", 1, [missing, damaged])], {
        problems: [{ code: "duplicate_section", line: 9, message: "" }],
      }),
    );
    expect(counts).toEqual({ missingId: 1, blocked: 1, fence: false, duplicateSection: true });
  });
});

describe("write failures", () => {
  const partial = (applied: unknown) => ({
    code: "backlog_partial_failure",
    message: "ledger append failed",
    details: { applied },
  });

  it("reads the entry a partly failed write saved", () => {
    expect(
      savedEntryId(partial({ file: { sha256: "c".repeat(64), entry_id: "bbbbbbbbbbbb" } })),
    ).toBe("bbbbbbbbbbbb");
    expect(savedEntryId(partial({}))).toBe("");
    expect(savedEntryId({ code: "backlog_file_conflict", message: "" })).toBe("");
  });

  it("says what was written, and never that nothing was when it is unknown", () => {
    expect(backlogErrorText(partial({}), tr)).toBe(
      "The result could not be confirmed. Reload to see where things stand before trying again.",
    );
    expect(backlogErrorText(partial({ file: { entry_id: "bbbbbbbbbbbb" } }), tr)).toBe(
      "Done so far: the file was saved. The next step is unconfirmed; reload to see where things stand before trying again.",
    );
  });

  it("names the reason the daemon gives", () => {
    const failure = (code: string, reason: string) => ({
      code,
      message: reason,
      details: { reason },
    });
    expect(backlogErrorText(failure("backlog_invalid_entry", "entry_not_found"), tr)).toBe(
      "That entry is not in the current backlog file.",
    );
    expect(backlogErrorText(failure("backlog_ambiguous", "ambiguous_section"), tr)).toContain(
      "Two sections have the same name.",
    );
    expect(backlogErrorText(failure("backlog_invalid_entry", "unterminated_fence"), tr)).toContain(
      "never closed",
    );
    expect(backlogErrorText(failure("backlog_invalid_entry", "structural_body"), tr)).toContain(
      "Use #### for headings",
    );
    expect(backlogErrorText({ code: "backlog_file_conflict", message: "" }, tr)).toContain(
      "nothing was written",
    );
  });
});
