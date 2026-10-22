import { useState } from "react";
import { useTranslation } from "react-i18next";
import { Button } from "../ui/button";
import { inputClass } from "../modals/settings/types";
import { canonicalGrokBotUrl } from "../../utils/webModelTargetDraft";
import { GrokActorPanel } from "./GrokActorPanel";

export function GrokActorSetup({
  groupId,
  actorId,
  isDark,
  isVisible = true,
  canAccessConnection = true,
  onOpenSharedSettings,
  draftUrl,
  onDraftChange,
  onEnabledChange,
  onUnresolvedChange,
  onBusyChange,
  saving = false,
}: {
  groupId: string;
  actorId: string;
  isDark: boolean;
  isVisible?: boolean;
  canAccessConnection?: boolean;
  onOpenSharedSettings?: () => void;
  draftUrl?: string;
  onDraftChange: (url: string | undefined) => void;
  onEnabledChange?: (enabled: boolean) => void;
  onUnresolvedChange?: (unresolved: boolean) => void;
  onBusyChange?: (busy: boolean) => void;
  saving?: boolean;
}) {
  const { t } = useTranslation("settings");
  const [editing, setEditing] = useState(false);
  return (
    <GrokActorPanel
      groupId={groupId}
      actorId={actorId}
      isVisible={isVisible}
      canAccessConnection={canAccessConnection}
      setup
      isDark={isDark}
      saving={saving}
      targetDirty={draftUrl !== undefined}
      onOpenSharedSettings={onOpenSharedSettings}
      onBusyChange={onBusyChange}
      onStateChange={(state) => {
        if (state) {
          onEnabledChange?.(state.enabled);
          onUnresolvedChange?.(Boolean(state.active_turn_id));
          if (
            draftUrl !== undefined &&
            state.bot_url &&
            canonicalGrokBotUrl(draftUrl) === state.bot_url
          )
            onDraftChange(undefined);
        }
      }}
      targetEditor={(state, locked) => {
        const saved = state?.bot_url || "";
        return (
          <>
            {saved && (
              <p className="break-all text-xs text-[var(--color-text-secondary)]">{saved}</p>
            )}
            {!saved || editing || draftUrl !== undefined ? (
              <div className="space-y-2">
                <label className="block text-xs">
                  {t("grokActor.url")}
                  <input
                    type="url"
                    required
                    value={draftUrl ?? saved}
                    onChange={(event) =>
                      onDraftChange(
                        canonicalGrokBotUrl(event.target.value) === saved
                          ? undefined
                          : event.target.value,
                      )
                    }
                    className={inputClass(isDark) + " mt-1 w-full"}
                    placeholder="https://grok.com/bot/…"
                    disabled={locked}
                  />
                </label>
                <p className="text-xs leading-relaxed text-[var(--color-text-muted)]">
                  {t(state?.active_turn_id ? "grokWebhook.waitForTask" : "grokActor.draftHint")}
                </p>
                {saved && (
                  <Button
                    variant="ghost"
                    disabled={locked}
                    onClick={() => {
                      setEditing(false);
                      onDraftChange(undefined);
                    }}
                  >
                    {t("common:cancel")}
                  </Button>
                )}
              </div>
            ) : (
              <Button variant="outline" disabled={locked} onClick={() => setEditing(true)}>
                {t("grokActor.change")}
              </Button>
            )}
          </>
        );
      }}
    />
  );
}
