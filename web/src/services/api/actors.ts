import type { Actor, ActorProfile, ActorProfileUsage } from "../../types";
import { actorProfileIdentityKey } from "../../utils/actorProfiles";
import {
  actorsReadOnlyRequestKey,
  apiForm,
  apiJson,
  ApiResponse,
  clearActorsReadOnlyRequest,
  clearGroupsReadRequest,
  reuseSharedReadRequest,
} from "./base";

export type ProfileView = "global" | "my" | "all";
export type ProfileScope = "global" | "user";

type ProfileLookupOptions = {
  scope?: ProfileScope;
  ownerId?: string;
};

type ProfileDeleteOptions = ProfileLookupOptions & {
  forceDetach?: boolean;
};

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
  if (init?.noCache || init?.signal) {
    return apiJson<{ actors: Actor[] }>(url, init);
  }
  if (includeUnread) {
    return apiJson<{ actors: Actor[] }>(url);
  }
  return reuseSharedReadRequest(
    actorsReadOnlyRequestKey(gid, includeInternal),
    () => apiJson<{ actors: Actor[] }>(url),
  );
}

export async function addActor(
  groupId: string,
  actorId: string,
  role: "peer" | "foreman",
  runtime: string,
  runner: "pty" | "headless",
  command: string,
  envPrivate?: Record<string, string>,
  options?: {
    profileId?: string;
    profileScope?: ProfileScope;
    profileOwner?: string;
    title?: string;
    capabilityAutoload?: string[];
    capabilityHidden?: string[];
  },
) {
  clearActorsReadOnlyRequest(groupId);
  clearGroupsReadRequest();
  return apiJson<{ actor: Actor }>(`/api/v1/groups/${encodeURIComponent(groupId)}/actors`, {
    method: "POST",
    body: JSON.stringify({
      actor_id: actorId,
      role,
      runner,
      runtime,
      command,
      env: {},
      env_private: envPrivate && Object.keys(envPrivate).length ? envPrivate : undefined,
      profile_id: options?.profileId || undefined,
      profile_scope: options?.profileScope || undefined,
      profile_owner: options?.profileOwner || undefined,
      capability_autoload: Array.isArray(options?.capabilityAutoload) ? options.capabilityAutoload : [],
      capability_hidden: Array.isArray(options?.capabilityHidden) ? options.capabilityHidden : [],
      title: options?.title || "",
      default_scope_key: "",
      by: "user",
    }),
  });
}

export async function updateActor(
  groupId: string,
  actorId: string,
  runtime?: string,
  runner?: "pty" | "headless",
  command?: string,
  title?: string,
  opts?: {
    profileId?: string;
    profileScope?: ProfileScope;
    profileOwner?: string;
    profileAction?: "convert_to_custom";
    enabled?: boolean;
    capabilityAutoload?: string[];
    capabilityHidden?: string[];
  },
) {
  clearActorsReadOnlyRequest(groupId);
  clearGroupsReadRequest();
  const body: Record<string, unknown> = { by: "user" };
  if (runtime !== undefined && runtime !== "") body.runtime = runtime;
  if (runner !== undefined) body.runner = runner;
  if (command !== undefined) body.command = command.trim();
  if (title !== undefined) body.title = title.trim();
  if (opts?.profileId !== undefined) body.profile_id = String(opts.profileId || "");
  if (opts?.profileScope !== undefined) body.profile_scope = String(opts.profileScope || "");
  if (opts?.profileOwner !== undefined) body.profile_owner = String(opts.profileOwner || "");
  if (opts?.profileAction) body.profile_action = opts.profileAction;
  if (typeof opts?.enabled === "boolean") body.enabled = opts.enabled;
  if (Array.isArray(opts?.capabilityAutoload)) body.capability_autoload = opts.capabilityAutoload;
  if (Array.isArray(opts?.capabilityHidden)) body.capability_hidden = opts.capabilityHidden;
  return apiJson(`/api/v1/groups/${encodeURIComponent(groupId)}/actors/${encodeURIComponent(actorId)}`, {
    method: "POST",
    body: JSON.stringify(body),
  });
}

export async function uploadActorAvatar(groupId: string, actorId: string, file: File) {
  clearActorsReadOnlyRequest(groupId);
  const form = new FormData();
  form.set("by", "user");
  form.set("file", file);
  return apiForm<{ actor: Actor }>(`/api/v1/groups/${encodeURIComponent(groupId)}/actors/${encodeURIComponent(actorId)}/avatar`, form);
}

export async function clearActorAvatar(groupId: string, actorId: string) {
  clearActorsReadOnlyRequest(groupId);
  return apiJson<{ actor: Actor }>(`/api/v1/groups/${encodeURIComponent(groupId)}/actors/${encodeURIComponent(actorId)}/avatar?by=user`, {
    method: "DELETE",
  });
}

export async function attachActorProfile(groupId: string, actorId: string, profileId: string, opts?: ProfileLookupOptions) {
  clearActorsReadOnlyRequest(groupId);
  clearGroupsReadRequest();
  return apiJson<{ actor: Actor }>(`/api/v1/groups/${encodeURIComponent(groupId)}/actors/${encodeURIComponent(actorId)}`, {
    method: "POST",
    body: JSON.stringify({
      by: "user",
      profile_id: profileId,
      profile_scope: opts?.scope || undefined,
      profile_owner: opts?.ownerId || undefined,
    }),
  });
}

export async function convertActorToCustom(groupId: string, actorId: string) {
  clearActorsReadOnlyRequest(groupId);
  clearGroupsReadRequest();
  return apiJson<{ actor: Actor }>(`/api/v1/groups/${encodeURIComponent(groupId)}/actors/${encodeURIComponent(actorId)}`, {
    method: "POST",
    body: JSON.stringify({ by: "user", profile_action: "convert_to_custom" }),
  });
}

export async function removeActor(groupId: string, actorId: string) {
  clearActorsReadOnlyRequest(groupId);
  clearGroupsReadRequest();
  return apiJson(
    `/api/v1/groups/${encodeURIComponent(groupId)}/actors/${encodeURIComponent(actorId)}?by=user`,
    { method: "DELETE" },
  );
}

export async function startActor(groupId: string, actorId: string) {
  clearActorsReadOnlyRequest(groupId);
  clearGroupsReadRequest();
  return apiJson(`/api/v1/groups/${encodeURIComponent(groupId)}/actors/${encodeURIComponent(actorId)}/start`, {
    method: "POST",
  });
}

export async function stopActor(groupId: string, actorId: string) {
  clearActorsReadOnlyRequest(groupId);
  clearGroupsReadRequest();
  return apiJson(`/api/v1/groups/${encodeURIComponent(groupId)}/actors/${encodeURIComponent(actorId)}/stop`, {
    method: "POST",
  });
}

export async function restartActor(groupId: string, actorId: string) {
  clearActorsReadOnlyRequest(groupId);
  clearGroupsReadRequest();
  return apiJson(
    `/api/v1/groups/${encodeURIComponent(groupId)}/actors/${encodeURIComponent(actorId)}/restart?by=user`,
    { method: "POST" },
  );
}

export async function newActorSession(groupId: string, actorId: string) {
  clearActorsReadOnlyRequest(groupId);
  clearGroupsReadRequest();
  return apiJson(
    `/api/v1/groups/${encodeURIComponent(groupId)}/actors/${encodeURIComponent(actorId)}/new_session?by=user`,
    { method: "POST" },
  );
}

export async function fetchActorPrivateEnvKeys(groupId: string, actorId: string) {
  return apiJson<{ group_id: string; actor_id: string; keys: string[]; masked_values?: Record<string, string> }>(
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
    {
      method: "POST",
      body: JSON.stringify({ by: "user", set: setVars, unset: unsetKeys, clear }),
    },
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
  return apiJson<{ deleted: boolean; profile_id: string; detached_count?: number; detached?: ActorProfileUsage[] }>(
    `/api/v1/profiles/${encodeURIComponent(profileId)}?${buildProfileDeleteQuery(opts)}`,
    { method: "DELETE" },
  );
}

export async function listActorProfiles(): Promise<ApiResponse<{ profiles: ActorProfile[] }>> {
  const [globalRes, myRes] = await Promise.all([listProfiles("global"), listProfiles("my")]);
  if (!globalRes.ok) return globalRes;
  if (!myRes.ok) return myRes;
  const seen = new Set<string>();
  const profiles: ActorProfile[] = [];
  for (const profile of [...globalRes.result.profiles, ...myRes.result.profiles]) {
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

export async function upsertActorProfile(profile: Record<string, unknown>, expectedRevision?: number) {
  return saveProfile({ ...profile, scope: "global", owner_id: "" }, expectedRevision);
}

export async function deleteActorProfile(profileId: string, opts?: { forceDetach?: boolean }) {
  return deleteProfile(profileId, { scope: "global", forceDetach: opts?.forceDetach });
}

export async function fetchActorProfilePrivateEnvKeys(profileId: string, opts?: ProfileLookupOptions) {
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
  return apiJson<{ profile_id: string; keys: string[] }>(`/api/v1/profiles/${encodeURIComponent(profileId)}/env_private`, {
    method: "POST",
    body: JSON.stringify({
      by: "user",
      scope: opts?.scope,
      owner_id: opts?.ownerId,
      set: setVars,
      unset: unsetKeys,
      clear,
    }),
  });
}

export async function copyActorPrivateEnvToProfile(
  profileId: string,
  groupId: string,
  actorId: string,
  opts?: ProfileLookupOptions,
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
      }),
    },
  );
}

export async function copyActorProfilePrivateEnvFromProfile(
  profileId: string,
  sourceProfileId: string,
  opts?: ProfileLookupOptions & {
    sourceScope?: ProfileScope;
    sourceOwnerId?: string;
  },
) {
  return copyProfilePrivateEnvFromProfile(profileId, sourceProfileId, {
    scope: "global",
    ...opts,
  });
}

export async function copyProfilePrivateEnvFromProfile(
  profileId: string,
  sourceProfileId: string,
  opts?: ProfileLookupOptions & {
    sourceScope?: ProfileScope;
    sourceOwnerId?: string;
  },
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
