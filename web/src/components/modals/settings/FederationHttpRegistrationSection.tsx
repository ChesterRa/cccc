import { useCallback, useMemo, useState } from "react";
import { useTranslation } from "react-i18next";

import * as api from "../../../services/api";
import type { FederationRegistration } from "../../../services/api/federation";
import type { GroupMeta } from "../../../types";
import {
  buildRegisterableOptions,
  canSubmitRegister,
  canVerify,
  initialRegisterSelection,
  registerableOptionsForDisplay,
  safeFederationErrorText,
  toggleGroupSelection,
} from "./federationRegistrationModel";
import {
  dangerButtonClass,
  inputClass,
  labelClass,
  primaryButtonClass,
  secondaryButtonClass,
  settingsWorkspaceBodyClass,
  settingsWorkspacePanelClass,
  settingsWorkspaceSoftPanelClass,
} from "./types";

interface FederationHttpRegistrationSectionProps {
  isDark: boolean;
  groups: GroupMeta[];
  registrations: FederationRegistration[];
  refreshStatus: () => Promise<void>;
}

export function FederationHttpRegistrationSection({
  isDark,
  groups,
  registrations,
  refreshStatus,
}: FederationHttpRegistrationSectionProps) {
  const { t } = useTranslation("settings");
  const [url, setUrl] = useState("");
  const [credential, setCredential] = useState("");
  const [remoteGroupId, setRemoteGroupId] = useState("");
  const [verified, setVerified] = useState(false);
  const [selected, setSelected] = useState<Set<string>>(initialRegisterSelection());
  const [error, setError] = useState("");
  const [busy, setBusy] = useState(false);

  const options = useMemo(() => buildRegisterableOptions(groups), [groups]);
  const shownOptions = registerableOptionsForDisplay(verified, options);

  const resetVerification = useCallback(() => {
    setVerified(false);
    setSelected(initialRegisterSelection());
  }, []);

  const onVerify = useCallback(async () => {
    setError("");
    setBusy(true);
    try {
      const probe = options[0]?.id || "";
      const resp = await api.verifyFederation({ groupId: probe, url, credentialRef: credential, remoteGroupId });
      if (resp.ok) {
        setVerified(true);
        setSelected(initialRegisterSelection());
      } else {
        setError(safeFederationErrorText(resp.error.message, credential));
      }
    } finally {
      setBusy(false);
    }
  }, [options, url, credential, remoteGroupId]);

  const onRegister = useCallback(async () => {
    setError("");
    setBusy(true);
    try {
      for (const gid of selected) {
        const resp = await api.registerFederation({ groupId: gid, url, credentialRef: credential, remoteGroupId });
        if (!resp.ok) {
          setError(safeFederationErrorText(resp.error.message, credential));
          break;
        }
      }
      await refreshStatus();
    } finally {
      setBusy(false);
    }
  }, [selected, url, credential, remoteGroupId, refreshStatus]);

  const onUnregister = useCallback(
    async (registrationId: string) => {
      setError("");
      setBusy(true);
      try {
        const resp = await api.unregisterFederation(registrationId);
        if (!resp.ok) setError(safeFederationErrorText(resp.error.message, credential));
        await refreshStatus();
      } finally {
        setBusy(false);
      }
    },
    [credential, refreshStatus],
  );

  const transport = "peer_cccc_http";
  const canVerifyNow = canVerify({ url, transport, remoteGroupId, busy });
  const canRegister = canSubmitRegister({
    verified,
    url,
    selectedCount: selected.size,
    busy,
    transport,
    remoteGroupId,
  });

  return (
    <div className={settingsWorkspaceBodyClass}>
      <div className={settingsWorkspacePanelClass(isDark)}>
        <div className="text-xs font-semibold uppercase tracking-wide text-[var(--color-text-muted)]">
          {t("federation.legacyHttpTitle")}
        </div>
        <p className="mt-2 text-xs text-[var(--color-text-muted)]">
          {t("federation.legacyHttpDescription")}
        </p>

        <div className="mt-4 space-y-4">
          <div>
            <label className={labelClass(isDark)}>{t("federation.remoteUrl")}</label>
            <input
              className={inputClass(isDark)}
              value={url}
              onChange={(e) => {
                setUrl(e.target.value);
                resetVerification();
              }}
              placeholder="https://peer.example"
            />
          </div>

          <div>
            <label className={labelClass(isDark)}>{t("federation.credentialReference")}</label>
            <input
              className={inputClass(isDark)}
              type="password"
              autoComplete="off"
              value={credential}
              onChange={(e) => {
                setCredential(e.target.value);
                resetVerification();
              }}
              placeholder="sec_remote_peer"
            />
          </div>

          <div>
            <label className={labelClass(isDark)}>{t("federation.remoteGroupId")}</label>
            <input
              className={inputClass(isDark)}
              value={remoteGroupId}
              onChange={(e) => {
                setRemoteGroupId(e.target.value);
                resetVerification();
              }}
              placeholder="g_remote"
            />
            <p className="mt-1 text-xs text-[var(--color-text-muted)]">
              {t("federation.remoteGroupHelp")}
            </p>
          </div>
        </div>

        <div className="mt-4 flex flex-wrap gap-2">
          <button type="button" className={secondaryButtonClass("md")} disabled={!canVerifyNow} onClick={onVerify}>
            {t("federation.verify")}
          </button>
          <button type="button" className={primaryButtonClass(busy)} disabled={!canRegister} onClick={onRegister}>
            {t("federation.registerSelected")}
          </button>
        </div>

        {verified && (
          <div className={`mt-4 ${settingsWorkspaceSoftPanelClass(isDark)}`}>
            <div className="text-xs font-semibold uppercase tracking-wide text-[var(--color-text-muted)]">
              {t("federation.registerableGroups")}
            </div>
            {shownOptions.length === 0 ? (
              <p className="mt-2 text-xs text-[var(--color-text-muted)]">{t("federation.noRegisterableGroups")}</p>
            ) : (
              <div className="mt-3 space-y-2">
                {shownOptions.map((opt) => (
                  <label key={opt.id} className="flex items-center gap-2.5 text-sm text-[var(--color-text-primary)]">
                    <input
                      type="checkbox"
                      className="h-4 w-4 rounded border-[var(--glass-border-subtle)]"
                      checked={selected.has(opt.id)}
                      onChange={() => setSelected((prev) => toggleGroupSelection(prev, opt.id))}
                    />
                    <span className="truncate">{opt.title}</span>
                  </label>
                ))}
              </div>
            )}
          </div>
        )}

        {error && <p className="mt-4 text-xs font-medium text-rose-600 dark:text-rose-400">{error}</p>}
      </div>

      <div className={settingsWorkspacePanelClass(isDark)}>
        <div className="text-xs font-semibold uppercase tracking-wide text-[var(--color-text-muted)]">{t("federation.registrations")}</div>
        {registrations.length === 0 ? (
          <p className="mt-3 text-xs text-[var(--color-text-muted)]">{t("federation.noneYet")}</p>
        ) : (
          <div className="mt-3 space-y-2">
            {registrations.map((reg) => (
              <div
                key={reg.registration_id}
                className="flex items-center justify-between gap-3 rounded-2xl border border-[var(--glass-border-subtle)] bg-[var(--color-bg-secondary)] px-4 py-3"
              >
                <div className="min-w-0">
                  <div className="truncate text-sm font-medium text-[var(--color-text-primary)]">
                    {reg.group_id} / {reg.url}
                  </div>
                  <div className="mt-0.5 text-[11px] text-[var(--color-text-muted)]">
                    {t("federation.status", { status: reg.status })}
                  </div>
                </div>
                <button
                  type="button"
                  className={dangerButtonClass("sm")}
                  disabled={busy}
                  onClick={() => onUnregister(reg.registration_id)}
                >
                  {t("federation.unregister")}
                </button>
              </div>
            ))}
          </div>
        )}
      </div>
    </div>
  );
}

export default FederationHttpRegistrationSection;
