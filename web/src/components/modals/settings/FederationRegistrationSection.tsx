import { useCallback, useEffect, useMemo, useState } from "react";

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
  settingsWorkspaceHeaderClass,
  settingsWorkspacePanelClass,
  settingsWorkspaceShellClass,
  settingsWorkspaceSoftPanelClass,
} from "./types";

interface FederationRegistrationSectionProps {
  isDark: boolean;
  isActive?: boolean;
}

// Self-contained Settings view: owns its own fetch/state so the Settings shell
// only needs to mount it as a sidebar tab.
export function FederationRegistrationSection({ isDark, isActive }: FederationRegistrationSectionProps) {
  const [url, setUrl] = useState("");
  const [credential, setCredential] = useState("");
  const [remoteGroupId, setRemoteGroupId] = useState("");
  const [verified, setVerified] = useState(false);
  const [groups, setGroups] = useState<GroupMeta[]>([]);
  const [selected, setSelected] = useState<Set<string>>(initialRegisterSelection());
  const [registrations, setRegistrations] = useState<FederationRegistration[]>([]);
  const [error, setError] = useState("");
  const [busy, setBusy] = useState(false);

  const options = useMemo(() => buildRegisterableOptions(groups), [groups]);
  const shownOptions = registerableOptionsForDisplay(verified, options);

  const refreshStatus = useCallback(async () => {
    const resp = await api.fetchFederationStatus();
    if (resp.ok) setRegistrations(resp.result.registrations || []);
  }, []);

  useEffect(() => {
    if (isActive === false) return undefined;
    let cancelled = false;
    void api.fetchGroups().then((resp) => {
      if (!cancelled && resp.ok) setGroups(resp.result.groups || []);
    });
    void refreshStatus();
    return () => {
      cancelled = true;
    };
  }, [isActive, refreshStatus]);

  const resetVerification = useCallback(() => {
    setVerified(false);
    setSelected(initialRegisterSelection());
  }, []);

  const onVerify = useCallback(async () => {
    setError("");
    setBusy(true);
    try {
      // No remote introspection in this stage: verify confirms URL + the
      // principal's authorization for a representative accessible group, then
      // reveals the registerable group checkboxes.
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

  // The UI only registers the peer_cccc_http transport, which requires a remote group id.
  const TRANSPORT = "peer_cccc_http";
  const canVerifyNow = canVerify({ url, transport: TRANSPORT, remoteGroupId, busy });
  const canRegister = canSubmitRegister({
    verified,
    url,
    selectedCount: selected.size,
    busy,
    transport: TRANSPORT,
    remoteGroupId,
  });

  return (
    <div className="space-y-5">
      <div className={settingsWorkspaceShellClass(isDark)}>
        <div className={settingsWorkspaceHeaderClass(isDark)}>
          <div className="min-w-0">
            <h3 className="text-sm font-semibold text-[var(--color-text-primary)]">Federation remote send</h3>
            <p className="mt-1 text-xs text-[var(--color-text-muted)]">
              Register a local group to a remote target, then send messages outbound through it.
            </p>
          </div>
        </div>

        <div className={settingsWorkspaceBodyClass}>
          <div className={settingsWorkspacePanelClass(isDark)}>
            <div className="text-xs font-semibold uppercase tracking-wide text-[var(--color-text-muted)]">
              Remote target
            </div>

            <div className="mt-4 space-y-4">
              <div>
                <label className={labelClass(isDark)}>Remote URL</label>
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
                <label className={labelClass(isDark)}>Credential reference</label>
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
                <label className={labelClass(isDark)}>Remote group id</label>
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
                  Required — the remote group on the target instance to deliver into.
                </p>
              </div>
            </div>

            <div className="mt-4 flex flex-wrap gap-2">
              <button type="button" className={secondaryButtonClass("md")} disabled={!canVerifyNow} onClick={onVerify}>
                Verify
              </button>
              <button type="button" className={primaryButtonClass(busy)} disabled={!canRegister} onClick={onRegister}>
                Register selected
              </button>
            </div>

            {verified && (
              <div className={`mt-4 ${settingsWorkspaceSoftPanelClass(isDark)}`}>
                <div className="text-xs font-semibold uppercase tracking-wide text-[var(--color-text-muted)]">
                  Registerable groups
                </div>
                {shownOptions.length === 0 ? (
                  <p className="mt-2 text-xs text-[var(--color-text-muted)]">No registerable groups.</p>
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

            {error && (
              <p className="mt-4 text-xs font-medium text-rose-600 dark:text-rose-400">{error}</p>
            )}
          </div>

          <div className={settingsWorkspacePanelClass(isDark)}>
            <div className="text-xs font-semibold uppercase tracking-wide text-[var(--color-text-muted)]">
              Registrations
            </div>
            {registrations.length === 0 ? (
              <p className="mt-3 text-xs text-[var(--color-text-muted)]">None yet.</p>
            ) : (
              <div className="mt-3 space-y-2">
                {registrations.map((reg) => (
                  <div
                    key={reg.registration_id}
                    className="flex items-center justify-between gap-3 rounded-2xl border border-[var(--glass-border-subtle)] bg-[var(--color-bg-secondary)] px-4 py-3"
                  >
                    <div className="min-w-0">
                      <div className="truncate text-sm font-medium text-[var(--color-text-primary)]">
                        {reg.group_id} → {reg.url}
                      </div>
                      <div className="mt-0.5 text-[11px] text-[var(--color-text-muted)]">status: {reg.status}</div>
                    </div>
                    <button
                      type="button"
                      className={dangerButtonClass("sm")}
                      disabled={busy}
                      onClick={() => onUnregister(reg.registration_id)}
                    >
                      Unregister
                    </button>
                  </div>
                ))}
              </div>
            )}
          </div>
        </div>
      </div>
    </div>
  );
}

export default FederationRegistrationSection;
