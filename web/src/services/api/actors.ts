import type { Actor, ActorProfile, ActorProfileUsage } from "../../types";
import { actorProfileIdentityKey } from "../../utils/actorProfiles";
import { getGroupBlobUrl } from "./groups";
import {
  actorsReadOnlyRequestKey,
  apiForm,
  apiJson,
  ApiResponse,
  clearActorsReadOnlyRequest,
  clearGroupsReadRequest,
  reuseSharedReadRequest,
} from "./base";

export type ProfileView = "global" | "my" | "all" | "accessible";
export type ProfileScope = "global" | "user";

export type ActorRuntimeActionResult = { actor?: Actor; runtime_confirmation_required?: boolean };

export type ActorRuntimeSwitchTarget =
  | { kind: "native"; source_id: string; expected_native_session_id: string }
  | {
      kind: "managed";
      patch: {
        runtime: string;
        runtime_mode?: "default" | "acp";
        command?: string | string[];
        env?: Record<string, string>;
        submit?: string;
        default_scope_key?: string;
      };
      profile_id?: string;
      profile_scope?: ProfileScope;
      profile_owner?: string;
      expected_profile_revision?: number;
      start?: boolean;
    };

export type ActorRuntimeSwitchRequest = {
  expected_actor_generation: string;
  expected_actor_updated_at: string;
  expected_source_id?: string;
  target: ActorRuntimeSwitchTarget;
};

export type ActorRuntimeSwitchResult = {
  group_id: string;
  actor_id: string;
  op_id: string;
  switched: true;
  from: { kind: "native" | "managed"; runtime: string; source_id?: string };
  to: { kind: "native" | "managed"; runtime: string; source_id?: string };
  completion: "done" | "pending";
  start: "started" | "pending" | "stopped" | "error" | "native" | "held";
  start_error?: unknown;
  access_paused?: boolean;
  notice_event_id?: string;
};

export type ActorRuntimeSwitchCompletion =
  | ActorRuntimeSwitchResult
  | {
      group_id: string;
      actor_id: string;
      switched: false;
      completion: "done";
      start?: undefined;
      access_paused?: undefined;
    };

type ProfileLookupOptions = { scope?: ProfileScope; ownerId?: string };

type ProfileDeleteOptions = ProfileLookupOptions & { forceDetach?: boolean };

function buildProfileQuery(opts?: ProfileLookupOptions): string {
  const params = new URLSearchParams();
  if (opts?.scope) params.set("scope", String(opts.scope));
  if (opts?.ownerId) params.set("owner_id", String(opts.ownerId));
  const query = params.toString();
  return query ? `?${query}` : "";
}

function buildProfileDeleteQuery(opts?: ProfileDeleteOptions): string {
  const params = new URLSearchParams();
  params.set("by", "user");
  if (opts?.scope) params.set("scope", String(opts.scope));
  if (opts?.ownerId) params.set("owner_id", String(opts.ownerId));
  if (opts?.forceDetach) params.set("force_detach", "true");
  return params.toString();
}

export function actorWithAvatar(groupId: string, actor: Actor): Actor {
  const avatarUrl = getGroupBlobUrl(groupId, actor.avatar_asset_path || "");
  const projected = { ...actor };
  if (avatarUrl) {
    projected.avatar_url = avatarUrl;
    projected.has_custom_avatar = true;
  } else {
    delete projected.avatar_url;
    delete projected.has_custom_avatar;
  }
  return projected;
}

export async function fetchActors(
  groupId: string,
  includeUnread = false,
  init?: RequestInit & { noCache?: boolean },
  options?: { includeInternal?: boolean },
) {
  const gid = String(groupId || "").trim();
  const includeInternal = Boolean(options?.includeInternal);
  const params = new URLSearchParams();
  if (includeUnread) params.set("include_unread", "true");
  if (includeInternal) params.set("include_internal", "true");
  const query = params.toString();
  const url = includeUnread
    ? `/api/v1/groups/${encodeURIComponent(gid)}/actors?${query}`
    : `/api/v1/groups/${encodeURIComponent(gid)}/actors${query ? `?${query}` : ""}`;
  const read = async () => {
    const response = await apiJson<{ actors: Actor[] }>(url, init);
    if (!response.ok) return response;
    return {
      ...response,
      result: {
        ...response.result,
        actors: response.result.actors.map((actor) => actorWithAvatar(gid, actor)),
      },
    };
  };
  if (init?.noCache || init?.signal || includeUnread) return read();
  return reuseSharedReadRequest(actorsReadOnlyRequestKey(gid, includeInternal), read);
}

function invalidateGroupActorsReads(groupId: string, includeGroups = true) {
  clearActorsReadOnlyRequest(groupId);
  if (includeGroups) clearGroupsReadRequest();
}

export async function addActor(
  groupId: string,
  actorId: string,
  role: "peer" | "foreman",
  runtime: string,
  command: string,
  envPrivate?: Record<string, string>,
  options?: {
    runtimeMode?: "default" | "acp";
    profileId?: string;
    profileScope?: ProfileScope;
    profileOwner?: string;
    title?: string;
    defaultScopeKey?: string;
    capabilityAutoload?: string[];
    capabilityHidden?: string[];
  },
) {
  invalidateGroupActorsReads(groupId);
  try {
    return await apiJson<{ actor: Actor }>(`/api/v1/groups/${encodeURIComponent(groupId)}/actors`, {
      method: "POST",
      body: JSON.stringify({
        actor_id: actorId,
        role,
        runtime,
        ...(options?.runtimeMode ? { runtime_mode: options.runtimeMode } : {}),
        command,
        env: {},
        env_private: envPrivate && Object.keys(envPrivate).length ? envPrivate : undefined,
        profile_id: options?.profileId || undefined,
        profile_scope: options?.profileScope || undefined,
        profile_owner: options?.profileOwner || undefined,
        capability_autoload: Array.isArray(options?.capabilityAutoload)
          ? options.capabilityAutoload
          : [],
        capability_hidden: Array.isArray(options?.capabilityHidden) ? options.capabilityHidden : [],
        title: options?.title || "",
        default_scope_key: options?.defaultScopeKey || "",
        by: "user",
      }),
    });
  } finally {
    invalidateGroupActorsReads(groupId);
  }
}

export async function updateActor(
  groupId: string,
  actorId: string,
  runtime?: string,
  command?: string,
  title?: string,
  opts?: {
    runtimeMode?: "default" | "acp";
    profileId?: string;
    profileScope?: ProfileScope;
    profileOwner?: string;
    profileAction?: "convert_to_custom";
    enabled?: boolean;
    capabilityAutoload?: string[];
    capabilityHidden?: string[];
    defaultScopeKey?: string;
  },
) {
  invalidateGroupActorsReads(groupId);
  const body: Record<string, unknown> = { by: "user" };
  if (runtime !== undefined && runtime !== "") body.runtime = runtime;
  if (command !== undefined) body.command = command.trim();
  if (opts?.runtimeMode) body.runtime_mode = opts.runtimeMode;
  if (title !== undefined) body.title = title.trim();
  if (opts?.profileId !== undefined) body.profile_id = String(opts.profileId || "");
  if (opts?.profileScope !== undefined) body.profile_scope = String(opts.profileScope || "");
  if (opts?.profileOwner !== undefined) body.profile_owner = String(opts.profileOwner || "");
  if (opts?.profileAction) body.profile_action = opts.profileAction;
  if (typeof opts?.enabled === "boolean") body.enabled = opts.enabled;
  if (Array.isArray(opts?.capabilityAutoload)) body.capability_autoload = opts.capabilityAutoload;
  if (Array.isArray(opts?.capabilityHidden)) body.capability_hidden = opts.capabilityHidden;
  if (opts?.defaultScopeKey !== undefined) body.default_scope_key = opts.defaultScopeKey;
  try {
    return await apiJson<ActorRuntimeActionResult>(
      `/api/v1/groups/${encodeURIComponent(groupId)}/actors/${encodeURIComponent(actorId)}`,
      { method: "POST", body: JSON.stringify(body) },
    );
  } finally {
    invalidateGroupActorsReads(groupId);
  }
}

export async function uploadActorAvatar(groupId: string, actorId: string, file: File) {
  invalidateGroupActorsReads(groupId, false);
  const form = new FormData();
  form.set("by", "user");
  form.set("file", file);
  try {
    return await apiForm<{ actor: Actor }>(
      `/api/v1/groups/${encodeURIComponent(groupId)}/actors/${encodeURIComponent(actorId)}/avatar`,
      form,
    );
  } finally {
    invalidateGroupActorsReads(groupId, false);
  }
}

export async function clearActorAvatar(groupId: string, actorId: string) {
  invalidateGroupActorsReads(groupId, false);
  try {
    return await apiJson<{ actor: Actor }>(
      `/api/v1/groups/${encodeURIComponent(groupId)}/actors/${encodeURIComponent(actorId)}/avatar?by=user`,
      { method: "DELETE" },
    );
  } finally {
    invalidateGroupActorsReads(groupId, false);
  }
}

export async function attachActorProfile(
  groupId: string,
  actorId: string,
  profileId: string,
  opts?: ProfileLookupOptions,
) {
  invalidateGroupActorsReads(groupId);
  try {
    return await apiJson<{ actor: Actor }>(
      `/api/v1/groups/${encodeURIComponent(groupId)}/actors/${encodeURIComponent(actorId)}`,
      {
        method: "POST",
        body: JSON.stringify({
          by: "user",
          profile_id: profileId,
          profile_scope: opts?.scope || undefined,
          profile_owner: opts?.ownerId || undefined,
        }),
      },
    );
  } finally {
    invalidateGroupActorsReads(groupId);
  }
}

export async function convertActorToCustom(groupId: string, actorId: string) {
  invalidateGroupActorsReads(groupId);
  try {
    return await apiJson<{ actor: Actor }>(
      `/api/v1/groups/${encodeURIComponent(groupId)}/actors/${encodeURIComponent(actorId)}`,
      { method: "POST", body: JSON.stringify({ by: "user", profile_action: "convert_to_custom" }) },
    );
  } finally {
    invalidateGroupActorsReads(groupId);
  }
}

export async function removeActor(groupId: string, actorId: string) {
  invalidateGroupActorsReads(groupId);
  try {
    return await apiJson(
      `/api/v1/groups/${encodeURIComponent(groupId)}/actors/${encodeURIComponent(actorId)}?by=user`,
      { method: "DELETE" },
    );
  } finally {
    invalidateGroupActorsReads(groupId);
  }
}

export async function startActor(groupId: string, actorId: string) {
  invalidateGroupActorsReads(groupId);
  try {
    return await apiJson<ActorRuntimeActionResult>(
      `/api/v1/groups/${encodeURIComponent(groupId)}/actors/${encodeURIComponent(actorId)}/start`,
      { method: "POST" },
    );
  } finally {
    invalidateGroupActorsReads(groupId);
  }
}

export async function stopActor(groupId: string, actorId: string) {
  invalidateGroupActorsReads(groupId);
  try {
    return await apiJson(
      `/api/v1/groups/${encodeURIComponent(groupId)}/actors/${encodeURIComponent(actorId)}/stop`,
      { method: "POST" },
    );
  } finally {
    invalidateGroupActorsReads(groupId);
  }
}

export async function restartActor(groupId: string, actorId: string) {
  invalidateGroupActorsReads(groupId);
  try {
    return await apiJson<ActorRuntimeActionResult>(
      `/api/v1/groups/${encodeURIComponent(groupId)}/actors/${encodeURIComponent(actorId)}/restart?by=user`,
      { method: "POST" },
    );
  } finally {
    invalidateGroupActorsReads(groupId);
  }
}

function runtimeSwitchUnknown<T>(response: ApiResponse<T>): ApiResponse<T> {
  if (
    !response.ok &&
    ([
      "NETWORK_ERROR",
      "PARSE_ERROR",
      "EMPTY_RESPONSE",
      "HTTP_ERROR",
      "UNKNOWN_ERROR",
      "daemon_unavailable",
    ].includes(response.error.code) ||
      response.error.httpStatus === 408 ||
      (response.error.httpStatus ?? 0) >= 500)
  ) {
    return {
      ok: false,
      error: {
        code: "actor_switch_unknown",
        message: "The result is unknown. Refresh the member before trying again.",
        details: { cause: response.error.code },
      },
    };
  }
  return response;
}

function isRuntimeSwitchResult(
  value: ActorRuntimeSwitchResult | undefined,
  groupId: string,
  actorId: string,
): value is ActorRuntimeSwitchResult {
  return Boolean(
    value &&
    value.switched === true &&
    value.group_id === groupId &&
    value.actor_id === actorId &&
    typeof value.op_id === "string" &&
    value.op_id.length > 0 &&
    ["done", "pending"].includes(value.completion) &&
    ["started", "pending", "stopped", "error", "native", "held"].includes(value.start) &&
    (value.completion === "pending" ? value.start === "held" : value.start !== "held") &&
    value.from &&
    ["native", "managed"].includes(value.from.kind) &&
    value.to &&
    ["native", "managed"].includes(value.to.kind),
  );
}

export async function switchActorRuntime(
  groupId: string,
  actorId: string,
  request: ActorRuntimeSwitchRequest,
): Promise<ApiResponse<ActorRuntimeSwitchResult>> {
  invalidateGroupActorsReads(groupId);
  try {
    const response = runtimeSwitchUnknown(
      await apiJson<ActorRuntimeSwitchResult>(
        `/api/v1/groups/${encodeURIComponent(groupId)}/actors/${encodeURIComponent(actorId)}/runtime_switch`,
        { method: "POST", body: JSON.stringify(request) },
      ),
    );
    if (response.ok && !isRuntimeSwitchResult(response.result, groupId, actorId)) {
      return {
        ok: false,
        error: {
          code: "actor_switch_unknown",
          message: "The result is unknown. Refresh the member before trying again.",
        },
      };
    }
    return response;
  } finally {
    invalidateGroupActorsReads(groupId);
  }
}

export async function completeActorRuntimeSwitch(
  groupId: string,
  actorId: string,
): Promise<ApiResponse<ActorRuntimeSwitchCompletion>> {
  invalidateGroupActorsReads(groupId);
  try {
    const response = runtimeSwitchUnknown(
      await apiJson<ActorRuntimeSwitchCompletion>(
        `/api/v1/groups/${encodeURIComponent(groupId)}/actors/${encodeURIComponent(actorId)}/runtime_switch/complete`,
        { method: "POST" },
      ),
    );
    if (
      response.ok &&
      !(
        response.result &&
        response.result.group_id === groupId &&
        response.result.actor_id === actorId &&
        ((response.result.switched === false && response.result.completion === "done") ||
          (response.result.switched === true &&
            isRuntimeSwitchResult(response.result, groupId, actorId)))
      )
    ) {
      return {
        ok: false,
        error: {
          code: "actor_switch_unknown",
          message: "The result is unknown. Refresh the member before trying again.",
        },
      };
    }
    return response;
  } finally {
    invalidateGroupActorsReads(groupId);
  }
}

export async function newActorSession(groupId: string, actorId: string) {
  invalidateGroupActorsReads(groupId);
  try {
    return await apiJson<ActorRuntimeActionResult>(
      `/api/v1/groups/${encodeURIComponent(groupId)}/actors/${encodeURIComponent(actorId)}/new_session?by=user`,
      { method: "POST" },
    );
  } finally {
    invalidateGroupActorsReads(groupId);
  }
}

export async function fetchActorPrivateEnvKeys(groupId: string, actorId: string) {
  return apiJson<{
    group_id: string;
    actor_id: string;
    keys: string[];
    masked_values?: Record<string, string>;
  }>(
    `/api/v1/groups/${encodeURIComponent(groupId)}/actors/${encodeURIComponent(actorId)}/env_private?by=user`,
  );
}

export async function updateActorPrivateEnv(
  groupId: string,
  actorId: string,
  setVars: Record<string, string>,
  unsetKeys: string[],
  clear: boolean,
) {
  return apiJson<{ group_id: string; actor_id: string; keys: string[] }>(
    `/api/v1/groups/${encodeURIComponent(groupId)}/actors/${encodeURIComponent(actorId)}/env_private`,
    { method: "POST", body: JSON.stringify({ by: "user", set: setVars, unset: unsetKeys, clear }) },
  );
}

export async function listProfiles(view: ProfileView = "global") {
  return apiJson<{ profiles: ActorProfile[] }>(`/api/v1/profiles?view=${encodeURIComponent(view)}`);
}

export async function getProfile(profileId: string, opts?: ProfileLookupOptions) {
  return apiJson<{ profile: ActorProfile; usage: ActorProfileUsage[] }>(
    `/api/v1/profiles/${encodeURIComponent(profileId)}${buildProfileQuery(opts)}`,
  );
}

export async function saveProfile(profile: Record<string, unknown>, expectedRevision?: number) {
  const profileId = String(profile.id || "").trim();
  if (!profileId) {
    const body: Record<string, unknown> = { by: "user", profile };
    if (typeof expectedRevision === "number") {
      body.expected_revision = Math.trunc(expectedRevision);
    }
    return apiJson<{ profile: ActorProfile }>(`/api/v1/actor_profiles`, {
      method: "POST",
      body: JSON.stringify(body),
    });
  }

  const body: Record<string, unknown> = { ...profile, by: "user" };
  if (typeof expectedRevision === "number") {
    body.expected_revision = Math.trunc(expectedRevision);
  }
  return apiJson<{ profile: ActorProfile }>(`/api/v1/profiles/${encodeURIComponent(profileId)}`, {
    method: "PUT",
    body: JSON.stringify(body),
  });
}

export async function deleteProfile(profileId: string, opts?: ProfileDeleteOptions) {
  return apiJson<{
    deleted: boolean;
    profile_id: string;
    detached_count?: number;
    detached?: ActorProfileUsage[];
  }>(`/api/v1/profiles/${encodeURIComponent(profileId)}?${buildProfileDeleteQuery(opts)}`, {
    method: "DELETE",
  });
}

export async function listActorProfiles(): Promise<ApiResponse<{ profiles: ActorProfile[] }>> {
  const accessibleRes = await listProfiles("accessible");
  if (!accessibleRes.ok) return accessibleRes;
  const seen = new Set<string>();
  const profiles: ActorProfile[] = [];
  for (const profile of accessibleRes.result.profiles) {
    const key = actorProfileIdentityKey(profile);
    if (!seen.has(key)) {
      seen.add(key);
      profiles.push(profile);
    }
  }
  return { ok: true, result: { profiles } };
}

export async function getActorProfile(profileId: string) {
  return getProfile(profileId);
}

export async function upsertActorProfile(
  profile: Record<string, unknown>,
  expectedRevision?: number,
) {
  return saveProfile({ ...profile, scope: "global", owner_id: "" }, expectedRevision);
}

export async function deleteActorProfile(profileId: string, opts?: { forceDetach?: boolean }) {
  return deleteProfile(profileId, { scope: "global", forceDetach: opts?.forceDetach });
}

export async function fetchActorProfilePrivateEnvKeys(
  profileId: string,
  opts?: ProfileLookupOptions,
) {
  return fetchProfilePrivateEnvKeys(profileId, opts ?? { scope: "global" });
}

export async function fetchProfilePrivateEnvKeys(profileId: string, opts?: ProfileLookupOptions) {
  return apiJson<{ profile_id: string; keys: string[]; masked_values?: Record<string, string> }>(
    `/api/v1/profiles/${encodeURIComponent(profileId)}/env_private${buildProfileQuery(opts)}${buildProfileQuery(opts) ? "&" : "?"}by=user`,
  );
}

export async function updateActorProfilePrivateEnv(
  profileId: string,
  setVars: Record<string, string>,
  unsetKeys: string[],
  clear: boolean,
) {
  return updateProfilePrivateEnv(profileId, setVars, unsetKeys, clear, { scope: "global" });
}

export async function updateProfilePrivateEnv(
  profileId: string,
  setVars: Record<string, string>,
  unsetKeys: string[],
  clear: boolean,
  opts?: ProfileLookupOptions,
) {
  return apiJson<{ profile_id: string; keys: string[] }>(
    `/api/v1/profiles/${encodeURIComponent(profileId)}/env_private`,
    {
      method: "POST",
      body: JSON.stringify({
        by: "user",
        scope: opts?.scope,
        owner_id: opts?.ownerId,
        set: setVars,
        unset: unsetKeys,
        clear,
      }),
    },
  );
}

export async function copyActorPrivateEnvToProfile(
  profileId: string,
  groupId: string,
  actorId: string,
  opts?: ProfileLookupOptions & {
    expectedActorGeneration?: string;
    expectedActorUpdatedAt?: string;
  },
) {
  return apiJson<{ profile_id: string; group_id: string; actor_id: string; keys: string[] }>(
    `/api/v1/actor_profiles/${encodeURIComponent(profileId)}/copy_actor_secrets`,
    {
      method: "POST",
      body: JSON.stringify({
        by: "user",
        scope: opts?.scope,
        owner_id: opts?.ownerId,
        group_id: groupId,
        actor_id: actorId,
        expected_actor_generation: opts?.expectedActorGeneration,
        expected_actor_updated_at: opts?.expectedActorUpdatedAt,
      }),
    },
  );
}

export async function copyVoiceAnalystPrivateEnvToProfile(profileId: string) {
  return apiJson<{ profile_id: string; keys: string[] }>(
    `/api/v1/actor_profiles/${encodeURIComponent(profileId)}/copy_voice_analyst_secrets`,
    { method: "POST", body: JSON.stringify({ by: "user", scope: "global", owner_id: "" }) },
  );
}

export async function copyVoiceSecretaryPrivateEnvToProfile(profileId: string) {
  return apiJson<{ profile_id: string; keys: string[] }>(
    `/api/v1/actor_profiles/${encodeURIComponent(profileId)}/copy_voice_secretary_secrets`,
    { method: "POST", body: JSON.stringify({ by: "user", scope: "global", owner_id: "" }) },
  );
}

export async function copyActorProfilePrivateEnvFromProfile(
  profileId: string,
  sourceProfileId: string,
  opts?: ProfileLookupOptions & { sourceScope?: ProfileScope; sourceOwnerId?: string },
) {
  return copyProfilePrivateEnvFromProfile(profileId, sourceProfileId, { scope: "global", ...opts });
}

export async function copyProfilePrivateEnvFromProfile(
  profileId: string,
  sourceProfileId: string,
  opts?: ProfileLookupOptions & { sourceScope?: ProfileScope; sourceOwnerId?: string },
) {
  return apiJson<{ profile_id: string; source_profile_id: string; keys: string[] }>(
    `/api/v1/profiles/${encodeURIComponent(profileId)}/copy_profile_secrets`,
    {
      method: "POST",
      body: JSON.stringify({
        by: "user",
        scope: opts?.scope,
        owner_id: opts?.ownerId,
        source_profile_id: sourceProfileId,
        source_scope: opts?.sourceScope ?? opts?.scope,
        source_owner_id: opts?.sourceOwnerId ?? opts?.ownerId,
      }),
    },
  );
}
