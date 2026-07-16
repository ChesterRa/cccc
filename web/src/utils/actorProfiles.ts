import type { ActorProfile } from "../types";

export function actorProfileIdentityKey(
  profile: Pick<ActorProfile, "id" | "scope" | "owner_id">,
): string {
  return JSON.stringify([
    String(profile.scope || "global").trim() || "global",
    String(profile.owner_id || "").trim(),
    String(profile.id || "").trim(),
  ]);
}

export function actorProfileMatchesRef(
  profile: Pick<ActorProfile, "id" | "scope" | "owner_id">,
  ref: { profileId?: string; profileScope?: string; profileOwner?: string },
): boolean {
  return (
    actorProfileIdentityKey(profile) ===
    actorProfileIdentityKey({
      id: String(ref.profileId || "").trim(),
      scope: (String(ref.profileScope || "global").trim() || "global") as ActorProfile["scope"],
      owner_id: String(ref.profileOwner || "").trim(),
    })
  );
}
