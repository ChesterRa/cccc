import { apiJson } from "./base";

/** An agent an update stopped or kept: who it is, the launch the host saw, and its title. */
export interface UpdateAgent {
  group_id: string;
  actor_id: string;
  actor_created_at?: string;
  generation?: string | null;
  owner_id?: string | null;
  title: string | null;
}

/** The agents an update stopped or kept, as the host found them once CCCC was back. */
export interface UpdateAgents {
  /** observing (still being looked for), observed, unconfirmed or not_observed. */
  state: string;
  reattached: UpdateAgent[];
  restarted: UpdateAgent[];
  not_back: UpdateAgent[];
  unconfirmed: UpdateAgent[];
}

/** An attended update's result, as the host recorded it. */
export interface LastUpdate {
  transaction: string;
  from_version: string | null;
  to_version: string;
  mode: string | null;
  /** installed, restarted, abandoned, not_started, unconfirmed, undone or kept. */
  result: string;
  stage: string | null;
  reason: string | null;
  finished_at: string;
  /** release or local. */
  source?: string | null;
  /** For a local restart, the identity token of the file it started. */
  token?: string | null;
  /** The build it moved to, as its self-check named it. */
  source_id?: string | null;
  agents?: UpdateAgents;
}

export interface UpdateStatus {
  current: string;
  /** standalone, pip, unmanaged or unrecognized. */
  installation: string;
  last: LastUpdate | null;
}

/** Why agents cannot keep running through a restart, for one member or for all of them. */
export interface KeepRefusal {
  code: string;
  /** Which member or session holder, when the refusal is about one. */
  group_id: string | null;
  actor_id: string | null;
  owner_id: string | null;
  display_name: string | null;
  /** CCCC's own sentence, shown only for a code this page does not know. */
  message: string;
}

/** Whether agents can keep running through a local restart, as checked now. */
export interface KeepPreview {
  allowed: boolean;
  refusals: KeepRefusal[];
}

/** A program checked for a local restart. */
export interface LocalCandidate {
  version: string;
  source_id: string;
  token: string;
  keep: KeepPreview;
}

/** What is now at the path the host was started from. */
export type LocalBuild =
  | (LocalCandidate &
      ({ state: "current"; transaction: string } | { state: "rebuilt"; built_at: string }))
  | { state: "incompatible"; version: string }
  /** The program at the launch path could not be checked. */
  | { state: "unreadable"; reason: string };

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
  /** Only a host that can carry out a local restart looks. */
  local: LocalBuild | null;
}

/** Local facts only; never asks release discovery. */
export async function fetchUpdateStatus() {
  return apiJson<{ status: UpdateStatus; in_place: boolean; local_restart?: boolean }>(
    "/api/v1/update/status",
    { signal: AbortSignal.timeout(10000) },
  );
}

/** Check the administrator's chosen target. */
export async function checkForUpdate(source: "release" | "local" = "release") {
  return apiJson<{
    check: UpdateCheck;
    in_place: boolean;
    local_restart: boolean;
    /** Whether keeping agents running is offered for a release; it is checked once downloaded. */
    release_keep?: boolean;
  }>(`/api/v1/update/check?source=${source}`, {
    method: "POST",
    signal: AbortSignal.timeout(60000),
  });
}

/** A published release, or the program at the path the host was started from. */
export type UpdateTarget =
  | { source: "release"; version: string }
  | { source: "local"; version: string; token: string; source_id: string; transaction?: string };

/** Whether running agents keep running or restart with CCCC. */
export type UpdateMode = "keep" | "restart";

/** Answers once the target is ready and the Web is closing for it. */
export async function startUpdate(target: UpdateTarget, mode: UpdateMode) {
  const body =
    target.source === "release"
      ? { version: target.version, mode }
      : { source: "local", token: target.token, source_id: target.source_id, mode };
  return apiJson<{ accepted: boolean; transaction: string; mode: string }>("/api/v1/update/start", {
    method: "POST",
    body: JSON.stringify(body),
  });
}
