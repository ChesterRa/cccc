import { useTranslation } from "react-i18next";
import { Switch } from "../../components/ui/switch";
import type { SessionNodeRelayStatus } from "../../services/api/sessionNodes";
import type { RelayToggleError } from "./useSessionNodes";

/** The instance-wide relay switch; pairing and settings show this same control. */
export function SessionNodeRelay({
  relay,
  relayBusy,
  relayError,
  busy,
  onToggleRelay,
}: {
  relay: SessionNodeRelayStatus;
  relayBusy: boolean;
  relayError: RelayToggleError;
  busy: boolean;
  onToggleRelay: (enabled: boolean) => void;
}) {
  const { t } = useTranslation("layout");
  const canEnableRelay = !!relay.public_url;
  const relayHintKey = !relay.enabled
    ? canEnableRelay
      ? "sessionNodes.publicRelayOffHint"
      : "sessionNodes.publicRelayRequiresRemoteAccess"
    : relay.effective
      ? "sessionNodes.publicRelayActiveHint"
      : "sessionNodes.publicRelayPendingHint";
  return (
    <section
      aria-label={t("sessionNodes.publicRelayTitle")}
      className="space-y-2 border-t border-[var(--glass-border-subtle)] pt-3"
    >
      <div className="flex flex-wrap items-center justify-between gap-2">
        <h4 className="text-sm font-medium">{t("sessionNodes.publicRelayTitle")}</h4>
        <Switch
          checked={relay.enabled}
          aria-checked={relay.enabled}
          aria-label={t("sessionNodes.publicRelayToggle")}
          aria-busy={relayBusy}
          disabled={busy || (!relay.enabled && !canEnableRelay)}
          onChange={(event) => onToggleRelay(event.currentTarget.checked)}
        />
      </div>
      {relay.public_url && (
        <p className="break-all text-xs text-[var(--color-text-secondary)]">
          {t("sessionNodes.publicRelayAddressLabel")}{" "}
          <bdi dir="ltr" className="font-mono text-[var(--color-text-primary)]">
            {relay.public_url}
          </bdi>
        </p>
      )}
      <p className="text-xs leading-relaxed text-[var(--color-text-secondary)]">
        {t(relayHintKey)}
      </p>
      {relayError && (
        <p role="alert" className="text-sm text-[var(--color-accent-danger)]">
          {t(
            relayError === "upgrade"
              ? "sessionNodes.upgradeRequired"
              : relayError === "unavailable"
                ? "sessionNodes.relayUnavailable"
                : "sessionNodes.relayToggleFailed",
          )}
        </p>
      )}
    </section>
  );
}
