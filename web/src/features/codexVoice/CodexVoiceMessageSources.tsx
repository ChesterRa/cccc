import { useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import {
  fetchVoiceNotifications,
  type VoiceNotificationSnapshot,
} from "../../services/api/codexVoice";
import { useGroupStore } from "../../stores/useGroupStore";
import type { CodexVoiceOutputStatus } from "./codexVoiceProviderChannel";

export function CodexVoiceMessageSources({
  active,
  onOpenSource,
  outputStatus,
}: {
  active: boolean;
  onOpenSource?: (groupId: string, eventId: string) => void;
  outputStatus?: CodexVoiceOutputStatus;
}) {
  const { t } = useTranslation("modals");
  const groups = useGroupStore((state) => state.groups);
  const [snapshot, setSnapshot] = useState<VoiceNotificationSnapshot | null>(null);
  const [error, setError] = useState(false);
  useEffect(() => {
    if (!active) return;
    let cancelled = false;
    let pending = false;
    const refresh = async () => {
      if (pending) return;
      pending = true;
      try {
        const response = await fetchVoiceNotifications();
        if (!cancelled) {
          setError(!response.ok);
          if (response.ok) setSnapshot(response.result);
        }
      } catch {
        if (!cancelled) setError(true);
      } finally {
        pending = false;
      }
    };
    void refresh();
    const timer = window.setInterval(() => void refresh(), 2000);
    return () => {
      cancelled = true;
      window.clearInterval(timer);
    };
  }, [active]);
  if (error)
    return (
      <p role="status" className="px-5 py-2 text-xs text-amber-700 dark:text-amber-300">
        {t("voicePreferences.sourceError")}
      </p>
    );
  if (!snapshot?.messages.length) return null;
  return (
    <details className="flex-none border-b border-[var(--glass-border-subtle)] px-5 py-2 text-xs text-[var(--color-text-secondary)]">
      <summary className="cursor-pointer">
        {t("voicePreferences.sources")}
        {snapshot.pending_count > 0
          ? ` · ${t("voicePreferences.pending", { count: snapshot.pending_count })}`
          : ""}
        {outputStatus && outputStatus.queued > 0
          ? ` · ${t("voicePreferences.queued", { count: outputStatus.queued })}`
          : snapshot.unconfirmed_count > 0
            ? ` · ${t("voicePreferences.unconfirmed", { count: snapshot.unconfirmed_count })}`
            : ""}
        {snapshot.suppressed_count > 0
          ? ` · ${t("voicePreferences.suppressed", { count: snapshot.suppressed_count })}`
          : ""}
      </summary>
      {outputStatus && outputStatus.queued > 0 ? (
        <p role="status" className="my-2">
          {t("voicePreferences.queued", { count: outputStatus.queued })}
          {outputStatus.blocked ? ` · ${t(`voicePreferences.wait_${outputStatus.blocked}`)}` : ""}
          {snapshot.unconfirmed_count > 0
            ? ` · ${t("voicePreferences.unconfirmed", { count: snapshot.unconfirmed_count })}`
            : ""}
        </p>
      ) : null}
      <p className="my-2 text-[var(--color-text-muted)]">{t("voicePreferences.sourceHint")}</p>
      <p className="mb-2 text-[var(--color-text-muted)]">{t("voicePreferences.deliveryHint")}</p>
      <ul className="max-h-36 space-y-2 overflow-y-auto pb-2">
        {snapshot.messages
          .slice()
          .reverse()
          .map((message) => (
            <li key={`${message.source.group_id}:${message.source.event_id}`}>
              <button
                type="button"
                className="text-left underline underline-offset-2 disabled:no-underline"
                disabled={!onOpenSource}
                onClick={() => onOpenSource?.(message.source.group_id, message.source.event_id)}
              >
                {t("voicePreferences.source", {
                  group:
                    groups.find((group) => group.group_id === message.source.group_id)?.title ||
                    message.source.group_id,
                  actor: message.by,
                })}
              </button>
              <p className="mt-0.5 text-[var(--color-text-muted)]">
                {t(
                  message.output_status === "suppressed" && message.suppression_reason
                    ? `voicePreferences.suppressed_${message.suppression_reason}`
                    : `voicePreferences.status_${message.output_status}`,
                )}
              </p>
            </li>
          ))}
      </ul>
    </details>
  );
}
