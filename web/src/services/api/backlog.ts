import type {
  BacklogEntry,
  BacklogProblem,
  BacklogSection,
  BacklogSnapshot,
  BacklogTaskLink,
  BacklogWriteResult,
} from "../../types";
import { apiJson, asRecord, asString, type ApiResponse } from "./base";

export type BacklogOperation =
  | "use"
  | "create"
  | "exclude"
  | "add"
  | "edit"
  | "move"
  | "close"
  | "promote";

/** An entry is named by its ID, or, until it has one, by its heading line in one file version. */
export type BacklogEntrySelector = { id: string } | { line: number };

function backlogPath(groupId: string, operation?: BacklogOperation): string {
  const base = `/api/v1/groups/${encodeURIComponent(groupId)}/backlog`;
  return operation ? `${base}/${operation}` : base;
}

function invalidResponse<T>(): ApiResponse<T> {
  return { ok: false, error: { code: "invalid_response", message: "Invalid backlog response" } };
}

function lineNumber(value: unknown): number {
  return typeof value === "number" && Number.isSafeInteger(value) && value >= 0 ? value : 0;
}

function problems(value: unknown): BacklogProblem[] {
  return (Array.isArray(value) ? value : []).flatMap((item) => {
    const record = asRecord(item);
    const code = asString(record?.code);
    return code
      ? [{ code, line: lineNumber(record?.line), message: asString(record?.message) }]
      : [];
  });
}

function taskLinks(value: unknown): BacklogTaskLink[] {
  return (Array.isArray(value) ? value : []).flatMap((item) => {
    const record = asRecord(item);
    const id = asString(record?.id).trim();
    if (!id) return [];
    const assignee = asString(record?.assignee).trim();
    return [
      {
        id,
        title: asString(record?.title),
        status: asString(record?.status).trim() || "planned",
        ...(assignee ? { assignee } : {}),
      },
    ];
  });
}

function entry(value: unknown): BacklogEntry | null {
  const record = asRecord(value);
  if (!record || typeof record.title !== "string" || typeof record.body !== "string") return null;
  const id = asString(record.id);
  return {
    id: id || null,
    title: record.title,
    line: lineNumber(record.line),
    body: record.body,
    problems: problems(record.problems),
    tasks: taskLinks(record.tasks),
  };
}

function section(value: unknown): BacklogSection | null {
  const record = asRecord(value);
  if (!record || typeof record.name !== "string" || !Array.isArray(record.entries)) return null;
  const entries = record.entries.map(entry);
  if (entries.some((item) => !item)) return null;
  return {
    name: record.name,
    line: lineNumber(record.line),
    closed: record.closed === true,
    entries: entries as BacklogEntry[],
  };
}

export function normalizeBacklogSnapshot(value: unknown): BacklogSnapshot | null {
  const record = asRecord(value);
  if (
    !record ||
    typeof record.configured !== "boolean" ||
    typeof record.scope_key !== "string" ||
    typeof record.scope_url !== "string" ||
    typeof record.path !== "string" ||
    typeof record.exists !== "boolean" ||
    typeof record.sha256 !== "string"
  ) {
    return null;
  }
  const sections = (Array.isArray(record.sections) ? record.sections : []).map(section);
  if (sections.some((item) => !item)) return null;
  const git = asRecord(record.git);
  return {
    configured: record.configured,
    scope_key: record.scope_key,
    scope_url: record.scope_url,
    path: record.path,
    exists: record.exists,
    sha256: record.sha256,
    git: git
      ? {
          repository: git.repository === true,
          tracked: git.tracked === true,
          ignored: git.ignored === true,
        }
      : null,
    problems: problems(record.problems),
    sections: sections as BacklogSection[],
  };
}

/** Reads the backlog of the scope the page shows; nothing is created or changed by a read. */
export async function fetchBacklog(
  groupId: string,
  scopeKey: string,
  scopeUrl: string,
): Promise<ApiResponse<BacklogSnapshot>> {
  const query = new URLSearchParams({ scope_key: scopeKey, scope_url: scopeUrl });
  const response = await apiJson<unknown>(`${backlogPath(groupId)}?${query}`);
  if (!response.ok) return response;
  const snapshot = normalizeBacklogSnapshot(response.result);
  if (!snapshot || snapshot.scope_key !== scopeKey || snapshot.scope_url !== scopeUrl) {
    return invalidResponse();
  }
  return { ok: true, result: snapshot };
}

/**
 * Every write names the scope and file it was read from; the daemon refuses it when either
 * changed since, or, with `if_file_sha256`, when the file did.
 */
export async function backlogOperation(
  groupId: string,
  operation: BacklogOperation,
  body: Record<string, unknown>,
): Promise<ApiResponse<BacklogWriteResult>> {
  const response = await apiJson<unknown>(backlogPath(groupId, operation), {
    method: "POST",
    body: JSON.stringify(body),
  });
  if (!response.ok) return response;
  const snapshot = normalizeBacklogSnapshot(response.result);
  if (!snapshot) return invalidResponse();
  const record = asRecord(response.result);
  const entryId = asString(record?.entry_id);
  const taskId = asString(record?.task_id);
  return {
    ok: true,
    result: {
      ...snapshot,
      ...(entryId ? { entry_id: entryId } : {}),
      ...(taskId ? { task_id: taskId, reused: record?.reused === true } : {}),
    },
  };
}
