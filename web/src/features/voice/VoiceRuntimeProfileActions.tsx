import { useState } from "react";
import { useTranslation } from "react-i18next";
import type { ActorProfile } from "../../types";
import { Button } from "../../components/ui/button";
import { ActorProfilesTab } from "../../components/modals/settings/ActorProfilesTab";

export function VoiceRuntimeProfileActions({
  form,
  name,
  onEditingChange,
}: {
  form: {
    mode: "custom" | "profile";
    selectedProfile?: ActorProfile;
    editingDisabled: boolean;
    settingsLoadFailed: boolean;
    profileSaving: boolean;
    settings: { runtime: string };
    acceptProfile: (profile: ActorProfile) => void;
    saveAsProfile: () => Promise<void>;
  };
  name: "Voice Secretary" | "Voice Analyst";
  onEditingChange?: (editing: boolean) => void;
}) {
  const { t } = useTranslation("settings");
  const { t: tActors } = useTranslation("actors");
  const [request, setRequest] = useState<{
    profile?: ActorProfile;
    nonce: number;
    defaultName: string;
  } | null>(null);
  const open = (profile?: ActorProfile) => {
    setRequest({ profile, nonce: Date.now(), defaultName: name });
    onEditingChange?.(true);
  };
  const close = () => {
    setRequest(null);
    onEditingChange?.(false);
  };
  return (
    <>
      {form.mode === "profile" ? (
        <>
          <Button
            variant="secondary"
            size="sm"
            disabled={!form.selectedProfile || form.editingDisabled}
            onClick={() => open(form.selectedProfile)}
          >
            {t("voiceSettings.editProfile")}
          </Button>
          <Button variant="ghost" size="sm" disabled={form.editingDisabled} onClick={() => open()}>
            {t("voiceSettings.createProfile")}
          </Button>
        </>
      ) : (
        <Button
          variant="secondary"
          disabled={form.editingDisabled || form.settingsLoadFailed || !form.settings.runtime}
          onClick={() => void form.saveAsProfile()}
        >
          {form.profileSaving ? t("voiceSettings.saving") : tActors("addToActorProfiles")}
        </Button>
      )}
      {request && (
        <ActorProfilesTab
          isDark={document.documentElement.classList.contains("dark")}
          isActive
          scope={request.profile?.scope === "user" ? "my" : "global"}
          editorOnly
          editorRequest={request}
          structuredOnly
          onEditorClose={close}
          onSaved={(profile) => {
            form.acceptProfile(profile);
            close();
          }}
        />
      )}
    </>
  );
}
