import { apiJson, type ApiResponse } from "./base";

/** A label captured with an input. It is not an authorization or a workspace path. */
export type ChetGroupContext = { group_id: string; group_title: string; scope_key: string | null };

export type ChetTextRequest = { text: string; context: ChetGroupContext | null };

export type ChetOutcome = {
  generation: string;
  turn_id: string;
  status: string;
  text: string;
  error: string;
};

/** One turn of a call, written once: when it finished, or as far as it got when the call ended. */
export type ChetTranscript = {
  call_id: string;
  turn_id: string;
  /** Order in which the call first saw this turn, from 1. */
  seq: number;
  role: "user" | "assistant";
  text: string;
  final_update: boolean;
  complete: boolean;
};

type Base = { id: string; created_at: string };

/**
 * Durable facts about the global assistant. A registered input is not proof that a
 * Runtime received it, and a voice delegation is the voice model's request, not the
 * user's words.
 */
export type ChetRecord =
  | (Base & { kind: "input"; request: ChetTextRequest })
  | (Base & {
      kind: "voice_delegation";
      delegation: { call_id: string; delegation_id: string; text: string };
    })
  | (Base & { kind: "receipt"; input_id: string; receipt: { generation: string; turn_id: string } })
  | (Base & { kind: "result"; input_ids: string[]; outcome: ChetOutcome })
  | (Base & { kind: "not_started"; input_id: string; reason: string })
  | (Base & { kind: "transcript"; transcript: ChetTranscript });

/** Live work the Web host can confirm right now; history holds everything else. */
export type ChetStatus = {
  generation: string | null;
  preparing: string[];
  queued: string[];
  working: string[];
  history_write_failed: boolean;
  /** The owner was busy, so only `preparing` is known; nothing else may be inferred. */
  owner_busy: boolean;
};

export type ChetPage = {
  records: ChetRecord[];
  /** Records this page could not show: malformed, or of a kind this build does not know. */
  skipped: number;
  /** Cursor for older records; null at the start of history. */
  before: string | null;
  /** Cursor for records appended after this page. */
  after: string;
  has_more: boolean;
};

export type ChetRegistration = "inserted" | "existing";

const isString = (value: unknown): value is string => typeof value === "string";
const isObject = (value: unknown): value is Record<string, unknown> =>
  !!value && typeof value === "object" && !Array.isArray(value);
const isStringArray = (value: unknown): value is string[] =>
  Array.isArray(value) && value.every(isString);

function isContext(value: unknown): value is ChetGroupContext | null {
  return (
    value === null ||
    (isObject(value) &&
      isString(value.group_id) &&
      isString(value.group_title) &&
      (value.scope_key === null || isString(value.scope_key)))
  );
}

function isTranscript(value: unknown): value is ChetTranscript {
  return (
    isObject(value) &&
    isString(value.call_id) &&
    isString(value.turn_id) &&
    Number.isSafeInteger(value.seq) &&
    (value.seq as number) > 0 &&
    (value.role === "user" || value.role === "assistant") &&
    isString(value.text) &&
    typeof value.final_update === "boolean" &&
    typeof value.complete === "boolean"
  );
}

/** Whether a value is a record this build can show. */
export function isChetRecord(value: unknown): value is ChetRecord {
  if (!isObject(value) || !isString(value.id) || !isString(value.created_at)) return false;
  switch (value.kind) {
    case "input":
      return (
        isObject(value.request) && isString(value.request.text) && isContext(value.request.context)
      );
    case "voice_delegation":
      return (
        isObject(value.delegation) &&
        isString(value.delegation.call_id) &&
        isString(value.delegation.delegation_id) &&
        isString(value.delegation.text)
      );
    case "receipt":
      return (
        isString(value.input_id) &&
        isObject(value.receipt) &&
        isString(value.receipt.generation) &&
        isString(value.receipt.turn_id)
      );
    case "result":
      return (
        isStringArray(value.input_ids) &&
        isObject(value.outcome) &&
        ["generation", "turn_id", "status", "text", "error"].every((key) =>
          isString((value.outcome as Record<string, unknown>)[key]),
        )
      );
    case "not_started":
      return isString(value.input_id) && isString(value.reason);
    case "transcript":
      return isTranscript(value.transcript);
    default:
      return false;
  }
}

function page(result: unknown): ChetPage | null {
  if (!isObject(result) || !Array.isArray(result.records) || !isString(result.after)) return null;
  if (!(result.before === undefined || result.before === null || isString(result.before)))
    return null;
  const records = result.records.filter(isChetRecord);
  return {
    records,
    skipped: result.records.length - records.length,
    before: isString(result.before) ? result.before : null,
    after: result.after,
    has_more: result.has_more === true,
  };
}

function invalid<T>(message: string): ApiResponse<T> {
  return { ok: false, error: { code: "invalid_response", message } };
}

/** The newest records, or the page before `before`. */
export async function fetchChetHistory(
  options: { before?: string; limit?: number } = {},
  signal?: AbortSignal,
): Promise<ApiResponse<ChetPage>> {
  const query = new URLSearchParams();
  if (options.before) query.set("before", options.before);
  if (options.limit) query.set("limit", String(options.limit));
  const suffix = query.size ? `?${query}` : "";
  const response = await apiJson<unknown>(`/api/v1/chet/history${suffix}`, { signal });
  if (!response.ok) return response;
  const parsed = page(response.result);
  return parsed ? { ok: true, result: parsed } : invalid("Chet history response is malformed");
}

/** Records appended after `after`. */
export async function fetchChetHistoryAfter(
  after: string,
  signal?: AbortSignal,
): Promise<ApiResponse<ChetPage>> {
  const response = await apiJson<unknown>(
    `/api/v1/chet/history?after=${encodeURIComponent(after)}`,
    { signal },
  );
  if (!response.ok) return response;
  const parsed = page(response.result);
  return parsed ? { ok: true, result: parsed } : invalid("Chet history response is malformed");
}

/** Every record about one input: the input itself and what followed it. Empty if unknown. */
export async function fetchChetInput(
  id: string,
  signal?: AbortSignal,
): Promise<ApiResponse<ChetRecord[]>> {
  const response = await apiJson<unknown>(`/api/v1/chet/inputs/${encodeURIComponent(id)}`, {
    signal,
  });
  if (!response.ok) return response;
  const records = isObject(response.result) ? response.result.records : undefined;
  // The host answers an unknown ID with `records: null`.
  if (records === null) return { ok: true, result: [] };
  if (!Array.isArray(records)) return invalid("Chet input response is malformed");
  return { ok: true, result: records.filter(isChetRecord) };
}

export async function fetchChetStatus(signal?: AbortSignal): Promise<ApiResponse<ChetStatus>> {
  const response = await apiJson<unknown>("/api/v1/chet/status", { signal });
  if (!response.ok) return response;
  const value = response.result;
  if (
    !isObject(value) ||
    !(value.generation === null || isString(value.generation)) ||
    !isStringArray(value.preparing) ||
    !isStringArray(value.queued) ||
    !isStringArray(value.working) ||
    typeof value.history_write_failed !== "boolean"
  ) {
    return invalid("Chet status response is malformed");
  }
  return {
    ok: true,
    result: {
      generation: value.generation,
      preparing: value.preparing,
      queued: value.queued,
      working: value.working,
      history_write_failed: value.history_write_failed,
      owner_busy: value.owner_busy === true,
    },
  };
}

/**
 * Registers one input. Only a first registration is delivered; the same ID again returns
 * `existing` and runs nothing. Any other outcome leaves the registration unknown.
 */
export async function submitChetInput(
  id: string,
  request: ChetTextRequest,
): Promise<ApiResponse<ChetRegistration>> {
  const response = await apiJson<unknown>("/api/v1/chet/inputs", {
    method: "POST",
    body: JSON.stringify({ id, request }),
  });
  if (!response.ok) return response;
  const registration = isObject(response.result) ? response.result.registration : undefined;
  return registration === "inserted" || registration === "existing"
    ? { ok: true, result: registration }
    : invalid("Chet registration response is malformed");
}

/**
 * Codes the Web host returns before it registers anything: invalid input, authorization,
 * read-only Web and the Voice start gate. Every other failure, including other 400s such
 * as an unknown history write and a lost response, leaves the registration unknown.
 */
const REFUSED_BEFORE_REGISTRATION = new Set([
  "invalid_request",
  "invalid_args",
  "unauthorized",
  "auth_required",
  "admin_required",
  "permission_denied",
  "read_only",
  "codex_voice_recovery_pending",
  "codex_voice_restarting",
]);

export function chetSubmissionRefused(code: string): boolean {
  return REFUSED_BEFORE_REGISTRATION.has(code);
}

/** The ID already names a different request; the original must be reviewed, not replaced. */
export const CHET_ID_CONFLICT = "chet_history_id_conflict";
