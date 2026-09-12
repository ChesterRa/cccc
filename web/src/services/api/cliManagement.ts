import type { AutomationRuleTrigger } from "../../types";
import { apiJson } from "./base";

export type CliOperation = "install" | "update" | "uninstall";
export type CliJob = {
  id: string;
  runtime: string;
  operation: CliOperation;
  status: "queued" | "running" | "succeeded" | "failed" | "interrupted";
  created_at: string;
  started_at: string | null;
  finished_at: string | null;
  source_rule: string | null;
  error: string | null;
};
export type CliInstallation = {
  version: string;
  executable: string;
  bin_paths: string[];
  installed_at: string;
};
export type CliSchedule = { id: string; enabled: boolean; trigger: AutomationRuleTrigger };
export type CliManagementState = {
  revision: number;
  rules: Array<CliSchedule & { next_run_at: string | null }>;
  installations: Record<string, CliInstallation>;
  jobs: Record<string, CliJob>;
};
export type CliManagementStatus = {
  runtimes: Array<{
    name: string;
    display_name: string;
    command: string;
    external_available: boolean;
    external_path: string | null;
    source:
      | { kind: "mise"; tool: string; node: boolean }
      | { kind: "official"; distribution: string }
      | { kind: "deepseek" }
      | { kind: "not_applicable"; reason: string };
    installation: CliInstallation | null;
    managed_error?: string | null;
    uninstall_available: boolean;
    uninstall_reason: string | null;
  }>;
  state: CliManagementState;
};
export type CliLogPage = {
  entries: Array<{ ts: string; stream: string; text: string }>;
  next_offset: number;
  has_more: boolean;
};

export function fetchCliManagement(history = false) {
  return apiJson<CliManagementStatus>(`/api/v1/cli-management${history ? "?history=true" : ""}`);
}

export function submitCliJob(runtime: string, operation: CliOperation, requestId: string) {
  return apiJson<{ job: CliJob }>("/api/v1/cli-management/jobs", {
    method: "POST",
    body: JSON.stringify({ runtime, operation, request_id: requestId }),
  });
}

export function saveCliSchedules(revision: number, rules: CliSchedule[]) {
  return apiJson<{ state: CliManagementState }>("/api/v1/cli-management/schedules", {
    method: "PUT",
    body: JSON.stringify({ revision, rules }),
  });
}

export function fetchCliJobLog(jobId: string, offset = 0) {
  return apiJson<CliLogPage>(
    `/api/v1/cli-management/jobs/${encodeURIComponent(jobId)}/log?offset=${offset}`,
  );
}
