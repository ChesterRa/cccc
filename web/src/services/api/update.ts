import { apiJson } from "./base";

/** An attended update's result, as the host recorded it. Agents are not observed. */
export interface LastUpdate {
  transaction: string;
  from_version: string | null;
  to_version: string;
  mode: string | null;
  /** installed, abandoned, not_started, unconfirmed, undone or kept. */
  result: string;
  stage: string | null;
  reason: string | null;
  finished_at: string;
}

export interface UpdateStatus {
  current: string;
  /** standalone, pip, unmanaged or unrecognized. */
  installation: string;
  last: LastUpdate | null;
}

export interface UpdateCheck {
  current: string;
  channel: string;
  installation: string;
  /** Absent when the check failed: whether an update exists is then unknown. */
  latest: string | null;
  /** newer, channel_switch, current, installed_newer or no_package. */
  status: string | null;
  failure: string | null;
  release_notes_url: string | null;
  /** The command that updates this installation by hand, when there is one. */
  command: string | null;
}

/** Local facts only; never asks release discovery. */
export async function fetchUpdateStatus() {
  return apiJson<{ status: UpdateStatus; in_place: boolean }>("/api/v1/update/status", {
    signal: AbortSignal.timeout(10000),
  });
}

/** One release check, because the administrator asked for it. */
export async function checkForUpdate() {
  return apiJson<{ check: UpdateCheck; in_place: boolean }>("/api/v1/update/check", {
    method: "POST",
    signal: AbortSignal.timeout(60000),
  });
}

/** Answers once the release is prepared and the Web is closing for the update. */
export async function startUpdate(version: string) {
  return apiJson<{ accepted: boolean; transaction: string; mode: string }>("/api/v1/update/start", {
    method: "POST",
    body: JSON.stringify({ version, mode: "restart" }),
  });
}
