// @vitest-environment happy-dom

import { act, useState } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, it, vi } from "vite-plus/test";
import type { ApiResponse } from "../../../src/services/api";
import type {
  BacklogEntry,
  BacklogSection,
  BacklogSnapshot,
  BacklogWriteResult,
  Task,
} from "../../../src/types";
import { BacklogPanel } from "../../../src/components/ContextModal/backlog/BacklogPanel";
import {
  todayLocal,
  type BacklogForm,
  type BacklogIntent,
} from "../../../src/components/ContextModal/backlog/backlogModel";
import type { BacklogState } from "../../../src/components/ContextModal/backlog/useBacklog";
import { createContextModalUi } from "../../../src/components/ContextModal/ui";

Object.assign(globalThis, { IS_REACT_ACT_ENVIRONMENT: true });

const tr = (_key: string, fallback: string, options?: Record<string, unknown>) =>
  fallback.replace(/\{\{(\w+)\}\}/g, (_match, name: string) => String(options?.[name] ?? ""));
const ui = createContextModalUi(false);

const SHA_A = "a".repeat(64);
const SHA_B = "b".repeat(64);
const SYNC_ID = "aaaaaaaaaaaa";

function entry(id: string | null, title: string, line: number, body: string): BacklogEntry {
  return { id, title, line, body, problems: [], tasks: [] };
}

function section(name: string, line: number, entries: BacklogEntry[]): BacklogSection {
  return { name, line, closed: false, entries };
}

function snapshot(
  sync = "Slow on big groups.\n",
  dark: BacklogEntry = entry(null, "Dark mode", 11, "Needs a palette.\n"),
  overrides: Partial<BacklogSnapshot> = {},
): BacklogSnapshot {
  return {
    configured: true,
    scope_key: "s_one",
    scope_url: "file:///repo",
    path: "BACKLOG.md",
    exists: true,
    sha256: SHA_A,
    git: { repository: true, tracked: false, ignored: true },
    problems: [],
    sections: [
      section("Now", 3, [entry(SYNC_ID, "Sync speed", 5, sync)]),
      section("Later", 9, [dark]),
    ],
    ...overrides,
  };
}

function written(snap: BacklogSnapshot, extra: Partial<BacklogWriteResult> = {}) {
  return { ok: true, result: { ...snap, ...extra } } as ApiResponse<BacklogWriteResult>;
}

function partialFailure(entryId: string) {
  return {
    ok: false,
    error: {
      code: "backlog_partial_failure",
      message: "ledger append failed",
      details: { applied: { file: { sha256: "f".repeat(64), entry_id: entryId } } },
    },
  } as ApiResponse<BacklogWriteResult>;
}

const outcomeTask: Task = {
  id: "T42",
  title: "Cache tasks",
  status: "active",
  outcome: "Halved the load time.\nMore notes.",
};

let host: HTMLDivElement;
let root: Root;
let form: BacklogForm | null;
let run: ReturnType<typeof vi.fn>;
let reload: ReturnType<typeof vi.fn>;
let onLinkSource: ReturnType<typeof vi.fn>;
let onArchiveTask: ReturnType<typeof vi.fn>;
let onTaskPromoted: ReturnType<typeof vi.fn>;
let dropForm: () => void;

function Harness({
  snap,
  intent,
  view,
}: {
  snap: BacklogSnapshot;
  intent: BacklogIntent | null;
  view: string;
}) {
  const [draft, setDraft] = useState<BacklogForm | null>(null);
  const [pending, setPending] = useState(intent);
  form = draft;
  dropForm = () => setDraft(null);
  const backlog: BacklogState = {
    snapshot: snap,
    loading: false,
    error: null,
    reload: reload as BacklogState["reload"],
    run: run as BacklogState["run"],
  };
  return (
    <BacklogPanel
      key={view}
      backlog={backlog}
      scopeState="ready"
      form={draft}
      setForm={setDraft}
      intent={pending}
      onIntentDone={() => setPending(null)}
      isDark={false}
      tr={tr}
      ui={ui}
      onOpenTask={vi.fn()}
      onTaskPromoted={onTaskPromoted as (taskId: string) => Promise<void>}
      onLinkSource={onLinkSource as never}
      onArchiveTask={onArchiveTask as (taskId: string) => Promise<string>}
    />
  );
}

async function render(snap: BacklogSnapshot, intent: BacklogIntent | null = null, view = "a") {
  await act(async () => root.render(<Harness snap={snap} intent={intent} view={view} />));
}

function button(label: string): HTMLButtonElement {
  const found = findButton(label);
  if (!found) throw new Error(`Missing button ${label}`);
  return found;
}

function findButton(label: string): HTMLButtonElement | undefined {
  return [...host.querySelectorAll("button")].find(
    (item) => item.textContent?.trim() === label || item.getAttribute("aria-label") === label,
  );
}

async function click(label: string) {
  await act(async () => button(label).click());
}

function type(input: HTMLInputElement | HTMLTextAreaElement, value: string) {
  const prototype =
    input instanceof HTMLTextAreaElement
      ? HTMLTextAreaElement.prototype
      : HTMLInputElement.prototype;
  act(() => {
    Object.getOwnPropertyDescriptor(prototype, "value")?.set?.call(input, value);
    input.dispatchEvent(new Event("input", { bubbles: true }));
  });
}

function editableText(): HTMLTextAreaElement {
  const found = [...host.querySelectorAll("textarea")].find((item) => !item.readOnly);
  if (!found) throw new Error("Missing editable text");
  return found;
}

const orphanNote = "The file or the entry this draft was for has changed";

beforeEach(() => {
  host = document.createElement("div");
  document.body.append(host);
  root = createRoot(host);
  form = null;
  run = vi.fn();
  reload = vi.fn(async () => null);
  onLinkSource = vi.fn(async (taskId: string, source: unknown) => ({
    task: { ...outcomeTask, id: taskId, source, rev: 9 },
  }));
  onArchiveTask = vi.fn(async () => "");
  onTaskPromoted = vi.fn(async () => {});
  Object.defineProperty(window, "confirm", { value: vi.fn(() => true), configurable: true });
});

afterEach(async () => {
  await act(async () => root.unmount());
  host.remove();
  vi.restoreAllMocks();
});

describe("BacklogPanel drafts", () => {
  it("carries a draft to a new file version while its entry is unchanged", async () => {
    await render(snapshot());
    await click("Sync speed");
    await click("Edit");
    type(editableText(), "My draft.\n");

    const elsewhere = entry(null, "Dark mode", 11, "Changed by someone else.\n");
    await render(snapshot(undefined, elsewhere, { sha256: SHA_B }));
    expect(host.textContent).not.toContain(orphanNote);

    run.mockResolvedValue(written(snapshot("My draft.\n", elsewhere, { sha256: "c".repeat(64) })));
    await click("Save");
    expect(run).toHaveBeenCalledWith("edit", {
      entry: { id: SYNC_ID },
      if_file_sha256: SHA_B,
      title: "Sync speed",
      body: "My draft.\n",
      scope_key: "s_one",
      scope_url: "file:///repo",
      path: "BACKLOG.md",
    });
  });

  it("keeps a draft whose entry changed for copying, and Reload does not unlock it", async () => {
    await render(snapshot());
    await click("Sync speed");
    await click("Edit");
    type(editableText(), "My draft.\n");

    const changed = snapshot("Fixed in part.\n", undefined, { sha256: SHA_B });
    await render(changed);
    expect(host.textContent).toContain(orphanNote);
    expect(findButton("Save")).toBeUndefined();
    const copy = [...host.querySelectorAll("textarea")].find((item) => item.readOnly);
    expect(copy?.value).toBe("Sync speed\n\nMy draft.");

    await click("Reload");
    expect(reload).toHaveBeenCalledTimes(1);
    await render({ ...changed });
    expect(host.textContent).toContain(orphanNote);
    expect(findButton("Save")).toBeUndefined();

    await click("Discard draft");
    expect(form).toBeNull();
    expect(host.textContent).not.toContain(orphanNote);
    expect(run).not.toHaveBeenCalled();
  });

  it("does not follow a draft to another file with the same text", async () => {
    await render(snapshot());
    await click("Sync speed");
    await click("Edit");
    type(editableText(), "My draft.\n");

    await render(snapshot(undefined, undefined, { path: "docs/BACKLOG.md" }));
    expect(host.textContent).toContain(orphanNote);
    expect(findButton("Save")).toBeUndefined();
    expect(run).not.toHaveBeenCalled();
  });

  it("leaves the next view's draft alone when a save returns after the view was replaced", async () => {
    await render(snapshot());
    await click("Sync speed");
    await click("Edit");
    type(editableText(), "My draft.\n");
    let answer: (response: ApiResponse<BacklogWriteResult>) => void = () => {};
    run.mockReturnValue(
      new Promise<ApiResponse<BacklogWriteResult>>((resolve) => {
        answer = resolve;
      }),
    );
    await click("Save");

    // The page moves to another Group, which starts without a draft, and one is begun there.
    await render(snapshot(), null, "b");
    await act(async () => dropForm());
    await click("Dark mode");
    await click("Edit");
    type(editableText(), "New draft.\n");

    await act(async () => answer(written(snapshot("My draft.\n", undefined, { sha256: SHA_B }))));
    expect(form).toMatchObject({ kind: "edit", title: "Dark mode", body: "New draft.\n" });
  });
});

describe("BacklogPanel and tasks", () => {
  it("puts a task without a source back as a new entry, then links it and offers to archive", async () => {
    const task = { ...outcomeTask, source: null };
    await render(snapshot(), { kind: "putBack", task });
    expect(host.textContent).toContain("T42 goes back as a new entry");
    const body = editableText();
    expect(body.value).toBe(`Outcome (${todayLocal()}): Halved the load time.\nMore notes.\n`);
    expect(body.value).not.toContain("T42");

    run.mockResolvedValue(written(snapshot(), { entry_id: "cccccccccccc" }));
    await click("Add entry");
    expect(run).toHaveBeenCalledWith("add", {
      section: "Now",
      title: "Cache tasks",
      body: body.value,
      scope_key: "s_one",
      scope_url: "file:///repo",
      path: "BACKLOG.md",
    });
    expect(onLinkSource).toHaveBeenCalledWith("T42", {
      scope_key: "s_one",
      path: "BACKLOG.md",
      entry_id: "cccccccccccc",
    });
    expect(host.textContent).toContain("T42 is back in the backlog. Archive the task too?");

    // Archived as it was once linked, so a later change is not taken over unseen.
    await click("Archive T42");
    expect(onArchiveTask).toHaveBeenCalledWith(expect.objectContaining({ id: "T42", rev: 9 }));
    expect(host.textContent).toContain("Archived T42.");
  });

  it("tries only the link again when it failed after the entry was saved", async () => {
    const task = { ...outcomeTask, source: null };
    onLinkSource.mockResolvedValueOnce({ error: "network down" });
    await render(snapshot(), { kind: "putBack", task });
    run.mockResolvedValue(written(snapshot(), { entry_id: "cccccccccccc" }));
    await click("Add entry");
    expect(host.textContent).toContain(
      "The entry was saved, but T42 could not be linked to it: network down",
    );
    expect(form).toBeNull();

    await click("Retry");
    expect(run).toHaveBeenCalledTimes(1);
    expect(onLinkSource).toHaveBeenCalledTimes(2);
    expect(onLinkSource).toHaveBeenLastCalledWith("T42", {
      scope_key: "s_one",
      path: "BACKLOG.md",
      entry_id: "cccccccccccc",
    });
    expect(host.textContent).not.toContain("could not be linked");
    expect(host.textContent).toContain("T42 is back in the backlog. Archive the task too?");
  });

  it("sends a task with a source only to that entry, and says when the file lacks it", async () => {
    const task = {
      ...outcomeTask,
      source: { scope_key: "s_one", path: "OTHER.md", entry_id: SYNC_ID },
    };
    await render(snapshot(), { kind: "putBack", task });
    expect(host.textContent).toContain("That entry is not in the current backlog file.");
    expect(findButton("Add entry")).toBeUndefined();
    expect(form).toBeNull();
    expect(onLinkSource).not.toHaveBeenCalled();
  });

  it("puts a task with a source back into its own entry without linking it again", async () => {
    const task = {
      ...outcomeTask,
      source: { scope_key: "s_one", path: "BACKLOG.md", entry_id: SYNC_ID },
    };
    await render(snapshot(), { kind: "putBack", task });
    const paragraph = `Outcome (${todayLocal()}): Halved the load time.\nMore notes.`;
    expect(editableText().value).toBe(`Slow on big groups.\n\n${paragraph}\n`);

    run.mockResolvedValue(written(snapshot(), { entry_id: SYNC_ID }));
    await click("Save");
    expect(run).toHaveBeenCalledWith("edit", expect.objectContaining({ entry: { id: SYNC_ID } }));
    expect(onLinkSource).not.toHaveBeenCalled();
    expect(host.textContent).toContain("T42 is back in the backlog. Archive the task too?");
  });

  it("takes a task's result to its entry from the banner", async () => {
    const task = {
      ...outcomeTask,
      source: { scope_key: "s_one", path: "BACKLOG.md", entry_id: SYNC_ID },
    };
    await render(snapshot(), { kind: "return", task });
    expect(host.textContent).toContain("Result of T42: Halved the load time.");

    await click("Add to the entry");
    expect(editableText().value).toBe(
      `Slow on big groups.\n\nOutcome (${todayLocal()}): Halved the load time.\nMore notes.\n`,
    );
    run.mockResolvedValue(written(snapshot(), { entry_id: SYNC_ID }));
    await click("Save");
    expect(onLinkSource).not.toHaveBeenCalled();
    expect(host.textContent).not.toContain("Archive the task too?");
  });

  it("shows the end of the text when a result is added on opening", async () => {
    const prototype = HTMLTextAreaElement.prototype;
    const original = Object.getOwnPropertyDescriptor(prototype, "scrollHeight");
    Object.defineProperty(prototype, "scrollHeight", { configurable: true, get: () => 480 });
    try {
      const task = {
        ...outcomeTask,
        source: { scope_key: "s_one", path: "BACKLOG.md", entry_id: SYNC_ID },
      };
      await render(snapshot(), { kind: "return", task });
      await click("Add to the entry");
      expect(editableText().scrollTop).toBe(480);

      await click("Cancel");
      await click("Edit");
      expect(editableText().scrollTop).toBe(0);
    } finally {
      if (original) Object.defineProperty(prototype, "scrollHeight", original);
      else delete (prototype as { scrollHeight?: number }).scrollHeight;
    }
  });
});

describe("BacklogPanel writes that fail part way", () => {
  it("does not add an entry twice once the file holds it", async () => {
    await render(snapshot());
    await click("Entry");
    const title = host.querySelector<HTMLInputElement>("form input");
    if (!title) throw new Error("Missing title");
    type(title, "New idea");

    run.mockResolvedValue(partialFailure("cccccccccccc"));
    await click("Add entry");
    expect(host.textContent).toContain("Done so far: the file was saved.");
    expect(findButton("Add entry")).toBeUndefined();
    expect(form).toBeNull();
    expect(run).toHaveBeenCalledTimes(1);
  });

  it("retries a promote with the saved ID and the same request", async () => {
    await render(snapshot());
    await click("Dark mode");
    await click("Create task…");
    run.mockResolvedValue(partialFailure("dddddddddddd"));
    await click("Create task");

    const first = run.mock.calls[0];
    expect(first[0]).toBe("promote");
    expect(first[1]).toMatchObject({ entry: { line: 11 }, if_file_sha256: SHA_A });
    const requestId = (first[1] as { task: { client_request_id: string } }).task.client_request_id;
    expect(requestId).toBeTruthy();
    expect(form).toMatchObject({ kind: "promote", entryId: "dddddddddddd", requestId });

    // Reloaded, the entry has the ID the failed write gave it and is otherwise unchanged.
    const withId = entry("dddddddddddd", "Dark mode", 11, "Needs a palette.\n");
    await render(snapshot(undefined, withId, { sha256: SHA_B }));
    run.mockResolvedValue(written(snapshot(undefined, withId), { task_id: "T50" }));
    await click("Create task");
    expect(run).toHaveBeenLastCalledWith("promote", {
      entry: { id: "dddddddddddd" },
      if_file_sha256: SHA_B,
      task: { title: "Dark mode", status: "planned", client_request_id: requestId },
      scope_key: "s_one",
      scope_url: "file:///repo",
      path: "BACKLOG.md",
    });
    expect(onTaskPromoted).toHaveBeenCalledWith("T50");
  });
});
