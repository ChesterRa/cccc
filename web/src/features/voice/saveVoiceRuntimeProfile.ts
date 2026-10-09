import {
  upsertActorProfile,
  updateProfilePrivateEnv,
  type copyVoiceAnalystPrivateEnvToProfile,
} from "../../services/api";
import { supportsAcpMode, type ActorProfile } from "../../types";
import type { VoiceAnalystDraftSettings } from "../codexVoice/codexVoiceAnalystSettingsModel";
import type { buildActorSecretSaveChanges } from "../../components/modals/actorSecretManagerModel";

export type VoiceProfileCheckpoint = { profile: ActorProfile; copied: boolean };

// Both voice identities use the same explicit Profile creation flow. Keep the
// destination after a partial failure so retry neither creates duplicates nor
// overwrites already copied credentials with a stale source snapshot.
export async function saveVoiceRuntimeProfile(
  name: string,
  settings: VoiceAnalystDraftSettings,
  environment: ReturnType<typeof buildActorSecretSaveChanges>,
  checkpoint: { current: VoiceProfileCheckpoint | null },
  copySecrets: typeof copyVoiceAnalystPrivateEnvToProfile,
): Promise<ActorProfile> {
  const response = await upsertActorProfile(
    {
      id: checkpoint.current?.profile.id,
      name: name.trim(),
      runtime: settings.runtime,
      ...(supportsAcpMode(settings.runtime) ? { runtime_mode: "acp" } : {}),
      command: settings.command.trim(),
      submit: "enter",
      env: {},
    },
    checkpoint.current?.profile.revision,
  );
  if (!response.ok) throw new Error(response.error.message);
  const profile = response.result.profile;
  if (!profile?.id) throw new Error("profile id is missing");
  checkpoint.current = { profile, copied: checkpoint.current?.copied || false };
  if (!checkpoint.current.copied) {
    const copied = await copySecrets(profile.id);
    if (!copied.ok) throw new Error(copied.error.message);
    checkpoint.current.copied = true;
  }
  if (
    environment.clear ||
    environment.unsetKeys.length ||
    Object.keys(environment.setVars).length
  ) {
    const updated = await updateProfilePrivateEnv(
      profile.id,
      environment.setVars,
      environment.unsetKeys,
      environment.clear,
      { scope: "global", ownerId: "" },
    );
    if (!updated.ok) throw new Error(updated.error.message);
  }
  checkpoint.current = null;
  return profile;
}
