import type { LocalSessionSource } from "../../types";
import { apiJson } from "./base";

export type SourceDiagnostic = {
  state:
    | "ready"
    | "shared_backend_missing"
    | "thread_not_loaded"
    | "receive_check_failed"
    | "unknown";
  checked_at: string | null;
  origin?: string;
};
export type SourceControl = {
  supported: boolean;
  management_revision: string;
  access_blocked: boolean;
  pause_complete: boolean;
  reason_code?: string;
};
export type SourceState = { control: SourceControl; diagnostic: SourceDiagnostic };
export type LocalSessionDirectory = {
  sources: LocalSessionSource[];
  removed_sources?: LocalSessionSource[];
  capabilities?: { source_control?: boolean; source_diagnostics?: boolean };
  source_states?: Record<string, SourceState>;
};
export type SourceControlResult = {
  source_id: string;
  management_revision: string;
  access_blocked: boolean;
  pause_complete: boolean;
  status: "removed" | "active" | "partial";
  error_code?: string;
  failed_group_ids?: string[];
};
export const controlLocalSource = (
  sourceId: string,
  action: "remove" | "restore",
  revision: string,
) =>
  apiJson<SourceControlResult>("/api/v1/local_sessions/control", {
    method: "POST",
    body: JSON.stringify({ source_id: sourceId, action, expected_revision: revision }),
  });
export const recheckLocalSource = (sourceId: string) =>
  apiJson<{
    source_id: string;
    diagnostic: SourceDiagnostic;
    check_status: "checked" | "cached" | "busy";
    retry_after_ms: number;
  }>("/api/v1/local_sessions/recheck", {
    method: "POST",
    body: JSON.stringify({ source_id: sourceId }),
  });
