import { useState } from "react";
import { useTranslation } from "react-i18next";

import { RefreshIcon } from "../../Icons";
import {
  membershipApprovalUrl,
  membershipManagementUrl,
  membershipPanelKind,
} from "./reachMembershipModel";
import {
  dangerButtonClass,
  primaryButtonClass,
  secondaryButtonClass,
  settingsWorkspaceBodyClass,
  settingsWorkspaceHeaderClass,
  settingsWorkspacePanelClass,
  settingsWorkspaceShellClass,
  settingsWorkspaceSoftPanelClass,
} from "./types";
import { useMembershipController } from "./useMembershipController";

interface AccountTabProps {
  isDark: boolean;
  isActive?: boolean;
  returnToWebAccess?: boolean;
  onOpenWebAccess: () => void;
}

type AccountViewKind = ReturnType<typeof membershipPanelKind> | "loading" | "unavailable";

function statusClass(kind: AccountViewKind): string {
  if (kind === "online") {
    return "border-emerald-500/30 bg-emerald-500/12 text-emerald-700 dark:text-emerald-300";
  }
  if (kind === "cut" || kind === "pending") {
    return "border-amber-500/30 bg-amber-500/12 text-amber-700 dark:text-amber-300";
  }
  return "border-[var(--glass-border-subtle)] bg-[var(--glass-tab-bg)] text-[var(--color-text-secondary)]";
}

export function AccountTab({
  isDark,
  isActive = true,
  returnToWebAccess = false,
  onOpenWebAccess,
}: AccountTabProps) {
  const { t, i18n } = useTranslation("settings");
  const [confirmDisconnect, setConfirmDisconnect] = useState(false);
  const {
    membership,
    membershipBusy,
    membershipError,
    membershipPollReady,
    refresh,
    connect,
    poll,
    disconnect,
  } = useMembershipController(isActive);
  const kind: AccountViewKind = membership
    ? membershipPanelKind(membership)
    : membershipBusy
      ? "loading"
      : membershipError
        ? "unavailable"
        : "loading";
  const language = i18n.resolvedLanguage || i18n.language;
  const approvalUrl = membershipApprovalUrl(membership, language);
  const managementUrl = membershipManagementUrl(membership, language);
  const accountOrigin = String(membership?.account_origin || "").trim();
  const reachLinked = Boolean(membership?.in_reach);
  const reachOnline = Boolean(membership?.online);
  const reachSupported = membership?.reach_supported !== false;
  const statusLabel = t(`account.status.${kind}`);

  const confirmRetirement = async () => {
    if (await disconnect()) setConfirmDisconnect(false);
  };

  return (
    <section className={settingsWorkspaceShellClass(isDark)}>
      <div className={settingsWorkspaceHeaderClass(isDark)}>
        <div className="min-w-0">
          <h3 className="text-sm font-semibold text-[var(--color-text-primary)]">
            {t("account.title")}
          </h3>
          <p className="mt-1 text-xs leading-5 text-[var(--color-text-muted)]">
            {t("account.description")}
          </p>
        </div>
        <button
          type="button"
          onClick={() => void refresh()}
          disabled={membershipBusy}
          className={secondaryButtonClass("sm")}
        >
          <RefreshIcon size={14} />
          {t("account.refresh")}
        </button>
      </div>

      <div className={settingsWorkspaceBodyClass}>
        <div className="flex flex-col gap-4 sm:flex-row sm:items-start sm:justify-between">
          <div className="min-w-0">
            <div className="flex flex-wrap items-center gap-2">
              <span
                className={`inline-flex rounded-full border px-2.5 py-1 text-xs font-medium ${statusClass(kind)}`}
                role="status"
                aria-live="polite"
              >
                {statusLabel}
              </span>
              {kind !== "logged_out" && accountOrigin ? (
                <span className="truncate text-xs text-[var(--color-text-muted)]">
                  {accountOrigin}
                </span>
              ) : null}
            </div>
            <p className="mt-3 max-w-3xl text-sm leading-6 text-[var(--color-text-secondary)]">
              {t(`account.stateHelp.${kind}`)}
            </p>
          </div>

          {kind === "logged_out" ? (
            <button
              type="button"
              onClick={() => void connect()}
              disabled={membershipBusy}
              className={`${primaryButtonClass(membershipBusy)} shrink-0`}
            >
              {t("account.linkInstallation")}
            </button>
          ) : null}
          {kind === "cut" ? (
            <button
              type="button"
              onClick={() => void connect()}
              disabled={membershipBusy}
              className={`${primaryButtonClass(membershipBusy)} shrink-0`}
            >
              {t("account.relinkInstallation")}
            </button>
          ) : null}
          {kind === "offline" || kind === "online" ? (
            <div className="flex shrink-0 flex-wrap gap-2">
              {managementUrl ? (
                <a
                  href={managementUrl}
                  target="_blank"
                  rel="noreferrer"
                  className={primaryButtonClass(false)}
                >
                  {t("account.manageAccount")}
                </a>
              ) : null}
            </div>
          ) : null}
        </div>

        {kind === "pending" && membership?.pending ? (
          <div className={settingsWorkspacePanelClass(isDark)}>
            <div className="flex flex-col gap-4 sm:flex-row sm:items-end sm:justify-between">
              <div>
                <div className="text-xs font-medium text-[var(--color-text-muted)]">
                  {t("account.deviceCode")}
                </div>
                <code className="mt-1 block select-all font-mono text-lg font-semibold tracking-[0.12em] text-[var(--color-text-primary)]">
                  {membership.pending.user_code || "—"}
                </code>
                <p className="mt-2 text-xs leading-5 text-[var(--color-text-muted)]">
                  {t("account.pendingSafety")}
                </p>
              </div>
              <div className="flex flex-wrap gap-2">
                {approvalUrl ? (
                  <a
                    href={approvalUrl}
                    target="_blank"
                    rel="noreferrer"
                    className={primaryButtonClass(false)}
                  >
                    {t("account.openApproval")}
                  </a>
                ) : null}
                <button
                  type="button"
                  onClick={() => void poll()}
                  disabled={membershipBusy || !membershipPollReady}
                  className={secondaryButtonClass()}
                >
                  {t("account.checkStatus")}
                </button>
                <button
                  type="button"
                  onClick={() => void disconnect()}
                  disabled={membershipBusy}
                  className={secondaryButtonClass()}
                >
                  {t("account.cancelLink")}
                </button>
              </div>
            </div>
          </div>
        ) : null}

        {membership?.account_reachable === false && membership?.logged_in ? (
          <p
            className="rounded-xl border border-amber-500/25 bg-amber-500/10 px-4 py-3 text-xs leading-5 text-amber-700 dark:text-amber-300"
            role="status"
          >
            {t("account.accountUnavailable")}
          </p>
        ) : null}

        {membershipError ? (
          <p
            className="rounded-xl border border-red-500/25 bg-red-500/10 px-4 py-3 text-xs leading-5 text-red-700 dark:text-red-300"
            role="alert"
          >
            {membershipError}
          </p>
        ) : null}

        {kind === "offline" || kind === "online" ? (
          <div className={settingsWorkspaceSoftPanelClass(isDark)}>
            <dl className="grid gap-4 sm:grid-cols-2">
              <div>
                <dt className="text-xs font-medium text-[var(--color-text-muted)]">
                  {t("account.installation")}
                </dt>
                <dd className="mt-1 break-all text-sm text-[var(--color-text-primary)]">
                  {membership?.device_id || t("account.currentInstallation")}
                </dd>
              </div>
              <div>
                <dt className="text-xs font-medium text-[var(--color-text-muted)]">
                  {t("account.accountService")}
                </dt>
                <dd className="mt-1 break-all text-sm text-[var(--color-text-primary)]">
                  {accountOrigin || "—"}
                </dd>
              </div>
            </dl>
          </div>
        ) : null}

        <div className={settingsWorkspacePanelClass(isDark)}>
          <div className="flex flex-col gap-3 sm:flex-row sm:items-center sm:justify-between">
            <div>
              <h4 className="text-sm font-semibold text-[var(--color-text-primary)]">
                {t("account.servicesTitle")}
              </h4>
              <p className="mt-1 text-xs leading-5 text-[var(--color-text-muted)]">
                {kind === "logged_out" || kind === "pending" || kind === "cut"
                  ? t("account.servicesNeedLink")
                  : kind === "loading"
                    ? t("account.servicesLoading")
                    : kind === "unavailable"
                      ? t("account.servicesUnavailable")
                      : !reachSupported
                        ? t("account.reachUnsupported")
                        : reachOnline
                          ? t("account.reachOnline")
                          : reachLinked
                            ? t("account.reachOff")
                            : t("account.reachAvailable")}
              </p>
            </div>
            <button type="button" onClick={onOpenWebAccess} className={secondaryButtonClass()}>
              {returnToWebAccess ? t("account.continueWebAccess") : t("account.openWebAccess")}
            </button>
          </div>
        </div>

        {kind === "offline" || kind === "online" ? (
          <div className="border-t border-[var(--glass-border-subtle)] pt-4">
            {confirmDisconnect ? (
              <div className="flex flex-col gap-3 sm:flex-row sm:items-center sm:justify-between">
                <p className="text-xs leading-5 text-[var(--color-text-secondary)]">
                  {t("account.disconnectPrompt")}
                </p>
                <div className="flex shrink-0 gap-2">
                  <button
                    type="button"
                    onClick={() => void confirmRetirement()}
                    disabled={membershipBusy}
                    className={dangerButtonClass()}
                  >
                    {t("account.confirmDisconnect")}
                  </button>
                  <button
                    type="button"
                    onClick={() => setConfirmDisconnect(false)}
                    disabled={membershipBusy}
                    className={secondaryButtonClass()}
                  >
                    {t("account.keepLinked")}
                  </button>
                </div>
              </div>
            ) : (
              <button
                type="button"
                onClick={() => setConfirmDisconnect(true)}
                disabled={membershipBusy}
                className={secondaryButtonClass("sm")}
              >
                {t("account.disconnect")}
              </button>
            )}
          </div>
        ) : null}
      </div>
    </section>
  );
}
