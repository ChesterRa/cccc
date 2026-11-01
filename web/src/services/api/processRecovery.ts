import { apiJson } from "./base";

/** Each owner keeps its own ledger of process records it could not confirm as ended. */
export type ProcessRecoveryOwner = "daemon" | "web";

export const PROCESS_RECOVERY_OWNERS: readonly ProcessRecoveryOwner[] = ["daemon", "web"];

export type ProcessRecoveryRecord = {
  id: string;
  /** Opaque; a retry or accept must send the version the user reviewed. */
  version: string;
  kind: string;
  created_at_unix_seconds: number | null;
  origin: string;
  reason: string;
  retryable: boolean;
  acceptance_supported: boolean;
};

export type ProcessRecoveryDiagnostic = { reason: string; hint: string; relative_path?: string };

export type ProcessRecoverySnapshot = {
  owner: ProcessRecoveryOwner;
  records: ProcessRecoveryRecord[];
  diagnostics: ProcessRecoveryDiagnostic[];
};

export type ProcessRecoveryRetryResult = {
  owner: ProcessRecoveryOwner;
  result: "cleared" | "still_pending";
  record: ProcessRecoveryRecord | null;
  diagnostics: ProcessRecoveryDiagnostic[];
};

export function isProcessRecoveryOwner(value: unknown): value is ProcessRecoveryOwner {
  return value === "daemon" || value === "web";
}

/** Owners named by a `codex_voice_recovery_pending` error, in a stable order. */
export function processRecoveryOwnersFromDetails(details: unknown): ProcessRecoveryOwner[] {
  const owners = (details as { recovery_owners?: unknown } | null | undefined)?.recovery_owners;
  if (!Array.isArray(owners)) return [];
  return PROCESS_RECOVERY_OWNERS.filter((owner) => owners.includes(owner));
}

const ownerPath = (owner: ProcessRecoveryOwner) => `/api/v1/process-recovery/${owner}`;

export function fetchProcessRecovery(owner: ProcessRecoveryOwner, signal?: AbortSignal) {
  return apiJson<ProcessRecoverySnapshot>(ownerPath(owner), { signal });
}

/** A lost response is not a failure: the retry may still finish, so list again. */
export function retryProcessRecovery(
  owner: ProcessRecoveryOwner,
  record: Pick<ProcessRecoveryRecord, "id" | "version">,
  signal?: AbortSignal,
) {
  return apiJson<ProcessRecoveryRetryResult>(`${ownerPath(owner)}/retry`, {
    method: "POST",
    body: JSON.stringify({ id: record.id, version: record.version }),
    signal,
  });
}

export function acceptProcessRecovery(
  owner: ProcessRecoveryOwner,
  record: Pick<ProcessRecoveryRecord, "id" | "version">,
  signal?: AbortSignal,
) {
  return apiJson<{ owner: ProcessRecoveryOwner; accepted: true }>(`${ownerPath(owner)}/accept`, {
    method: "POST",
    body: JSON.stringify({ id: record.id, version: record.version }),
    signal,
  });
}
