import { useState } from "react";
import { useTranslation } from "react-i18next";

import { copyTextToClipboard } from "../../../utils/copy";
import {
  hostnameLooksTokenless,
  membershipAdminWebUrl,
  membershipApprovalUrl,
  membershipPanelKind,
  membershipPublicAddress,
  type MembershipState,
} from "./reachMembershipModel";
import { primaryButtonClass, secondaryButtonClass } from "./types";

interface ReachMembershipSectionProps {
  membership: MembershipState | null;
  membershipBusy: boolean;
  membershipError: string;
  membershipPollReady: boolean;
  hasAdminToken: boolean;
  reachBusy: boolean;
  reachAction: "starting" | "stopping" | null;
  onConnectAccount: () => void;
  onPollAccount: () => void;
  onOpenAccount: () => void;
  onCreateAdminToken: () => void;
  onCreateWebLogin: () => Promise<string>;
  onReachOn: () => void;
  onReachOff: () => void;
  onCopied: () => void;
  onCopyFailed: () => void;
}

export function ReachMembershipSection({
  membership,
  membershipBusy,
  membershipError,
  membershipPollReady,
  hasAdminToken,
  reachBusy,
  reachAction,
  onConnectAccount,
  onPollAccount,
  onOpenAccount,
  onCreateAdminToken,
  onCreateWebLogin,
  onReachOn,
  onReachOff,
  onCopied,
  onCopyFailed,
}: ReachMembershipSectionProps) {
  const { t, i18n } = useTranslation("settings");
  const [copied, setCopied] = useState<"public" | "admin" | null>(null);
  const [webLoginBusy, setWebLoginBusy] = useState(false);
  const kind = membership
    ? membershipPanelKind(membership)
    : membershipBusy
      ? "loading"
      : membershipError
        ? "unavailable"
        : "loading";
  const language = i18n.resolvedLanguage || i18n.language;
  const approvalUrl = membershipApprovalUrl(membership, language);
  const publicAddress = membershipPublicAddress(membership);
  const adminWebUrl = membershipAdminWebUrl(membership);
  const hostname = String(membership?.hostname || "").trim();
  const unsafeHostname = Boolean(hostname) && !hostnameLooksTokenless(hostname);
  const pendingCode = String(membership?.pending?.user_code || "").trim();
  const reachSupported = membership?.reach_supported !== false;
  const canStop = kind === "online" || Boolean(membership?.in_reach);
  const visibleError = membershipError;

  const statusLabel =
    reachAction === "starting"
      ? t("webAccess.reach.statusStarting")
      : reachAction === "stopping"
        ? t("webAccess.reach.statusStopping")
        : !reachSupported && (kind === "offline" || kind === "online")
          ? t("webAccess.reach.statusUnsupported")
          : kind === "online"
            ? t("webAccess.reach.statusOnline")
            : kind === "cut"
              ? t("webAccess.reach.statusCut")
              : kind === "offline"
                ? t("webAccess.reach.statusOffline")
                : kind === "pending"
                  ? t("webAccess.reach.statusPending")
                  : kind === "unavailable"
                    ? t("webAccess.reach.statusUnavailable")
                    : kind === "loading"
                      ? t("webAccess.reach.statusLoading")
                      : t("webAccess.reach.statusLoggedOut");

  const stateHelp =
    kind === "logged_out"
      ? t("webAccess.reach.loggedOut")
      : kind === "pending"
        ? t("webAccess.reach.pending")
        : kind === "cut"
          ? t("webAccess.reach.cut")
          : kind === "loading"
            ? t("webAccess.reach.loading")
            : kind === "unavailable"
              ? t("webAccess.reach.loadFailed")
              : !reachSupported
                ? t("webAccess.reach.unsupported")
                : kind === "online"
                  ? t("webAccess.reach.online")
                  : !hasAdminToken
                    ? t("webAccess.reach.adminTokenRequired")
                    : t("webAccess.reach.loggedInOffline");

  const copyValue = async (id: "public" | "admin", value: string) => {
    const ok = await copyTextToClipboard(value);
    if (!ok) {
      onCopyFailed();
      return;
    }
    setCopied(id);
    window.setTimeout(() => setCopied((current) => (current === id ? null : current)), 1500);
    onCopied();
  };

  const openAdminWeb = async () => {
    if (webLoginBusy) return;
    const popup = window.open("about:blank", "_blank");
    if (popup) popup.opener = null;
    setWebLoginBusy(true);
    try {
      const url = await onCreateWebLogin();
      if (!url) {
        popup?.close();
        return;
      }
      if (popup) popup.location.replace(url);
      else window.open(url, "_blank", "noopener,noreferrer");
    } finally {
      setWebLoginBusy(false);
    }
  };

  const copyAdminWeb = async () => {
    if (webLoginBusy) return;
    setWebLoginBusy(true);
    try {
      const url = await onCreateWebLogin();
      if (url) await copyValue("admin", url);
    } finally {
      setWebLoginBusy(false);
    }
  };

  return (
    <div className="rounded-xl border border-[var(--glass-border-subtle)] bg-[var(--glass-panel-bg)] p-4">
      <div className="flex flex-col gap-4 sm:flex-row sm:items-start sm:justify-between">
        <div className="min-w-0">
          <div className="text-xs font-semibold uppercase tracking-wide text-[var(--color-text-muted)]">
            {t("webAccess.reach.title")}
          </div>
          <div
            className="mt-1 text-sm font-medium text-[var(--color-text-primary)]"
            role="status"
            aria-live="polite"
          >
            {statusLabel}
          </div>
          <p className="mt-1 max-w-3xl text-xs leading-5 text-[var(--color-text-muted)]">
            {stateHelp}
          </p>
        </div>

        <div className="flex shrink-0 flex-wrap gap-2">
          {kind === "logged_out" ? (
            <button
              type="button"
              onClick={onConnectAccount}
              disabled={membershipBusy}
              className={primaryButtonClass(membershipBusy)}
            >
              {t("webAccess.reach.setup")}
            </button>
          ) : null}
          {kind === "cut" ? (
            <button
              type="button"
              onClick={onConnectAccount}
              disabled={membershipBusy}
              className={primaryButtonClass(membershipBusy)}
            >
              {t("webAccess.reach.relink")}
            </button>
          ) : null}
          {kind === "pending" && approvalUrl ? (
            <a
              href={approvalUrl}
              target="_blank"
              rel="noreferrer"
              className={primaryButtonClass(false)}
            >
              {t("webAccess.reach.openApproval")}
            </a>
          ) : null}
          {kind === "pending" ? (
            <button
              type="button"
              onClick={onPollAccount}
              disabled={membershipBusy || !membershipPollReady}
              className={secondaryButtonClass()}
            >
              {t("webAccess.reach.checkAgain")}
            </button>
          ) : null}
          {kind === "offline" && reachSupported && !hasAdminToken ? (
            <button
              type="button"
              onClick={onCreateAdminToken}
              disabled={membershipBusy}
              className={primaryButtonClass(membershipBusy)}
            >
              {t("webAccess.reach.createAdminToken")}
            </button>
          ) : null}
          {kind === "offline" && reachSupported && hasAdminToken ? (
            <button
              type="button"
              onClick={onReachOn}
              disabled={reachBusy || membershipBusy}
              className={primaryButtonClass(reachBusy)}
            >
              {t("webAccess.reach.start")}
            </button>
          ) : null}
          {kind === "online" && adminWebUrl ? (
            <button
              type="button"
              onClick={() => void openAdminWeb()}
              disabled={webLoginBusy}
              className={primaryButtonClass(webLoginBusy)}
            >
              {t("webAccess.reach.openWeb")}
            </button>
          ) : null}
          {canStop ? (
            <button
              type="button"
              onClick={onReachOff}
              disabled={reachBusy || membershipBusy}
              className={secondaryButtonClass()}
            >
              {t("webAccess.reach.stop")}
            </button>
          ) : null}
          {kind === "offline" || kind === "online" || kind === "unavailable" ? (
            <button type="button" onClick={onOpenAccount} className={secondaryButtonClass()}>
              {t("webAccess.reach.manageAccount")}
            </button>
          ) : null}
        </div>
      </div>

      {visibleError ? (
        <p className="mt-3 text-xs leading-5 text-red-600 dark:text-red-300" role="alert">
          {visibleError}
        </p>
      ) : null}

      {kind === "pending" ? (
        <div className="mt-4 border-t border-[var(--glass-border-subtle)] pt-4">
          <div className="text-xs text-[var(--color-text-muted)]">
            {t("webAccess.reach.pendingCode")}
          </div>
          <code className="mt-1 block select-all font-mono text-base font-semibold tracking-[0.12em] text-[var(--color-text-primary)]">
            {pendingCode || "—"}
          </code>
        </div>
      ) : null}

      {kind === "online" ? (
        <div className="mt-4 space-y-4 border-t border-[var(--glass-border-subtle)] pt-4">
          <div className="flex flex-col gap-3 sm:flex-row sm:items-center sm:justify-between">
            <div className="min-w-0">
              <div className="text-xs font-medium text-[var(--color-text-primary)]">
                {t("webAccess.reach.publicAddressLabel")}
              </div>
              <div className="mt-1 text-xs leading-5 text-[var(--color-text-muted)]">
                {publicAddress
                  ? t("webAccess.reach.publicAddressHelp")
                  : t("webAccess.reach.publicAddressMissing")}
              </div>
              {publicAddress ? (
                <code className="mt-2 block break-all font-mono text-xs text-[var(--color-text-secondary)]">
                  {publicAddress}
                </code>
              ) : null}
            </div>
            {publicAddress ? (
              <button
                type="button"
                onClick={() => void copyValue("public", publicAddress)}
                className={`${secondaryButtonClass("sm")} shrink-0`}
              >
                {copied === "public" ? t("webAccess.reach.copied") : t("webAccess.reach.copy")}
              </button>
            ) : null}
          </div>

          {unsafeHostname ? (
            <p className="text-xs leading-5 text-amber-700 dark:text-amber-300">
              {t("webAccess.reach.hostnameUnsafe")}
            </p>
          ) : null}

          {adminWebUrl ? (
            <details className="text-xs text-[var(--color-text-secondary)]">
              <summary className="cursor-pointer font-medium text-[var(--color-text-primary)]">
                {t("webAccess.reach.adminAccessSummary")}
              </summary>
              <p className="mt-2 max-w-3xl leading-5 text-amber-700 dark:text-amber-300">
                {t("webAccess.reach.adminAccessWarning")}
              </p>
              <button
                type="button"
                onClick={() => void copyAdminWeb()}
                disabled={webLoginBusy}
                className={`${secondaryButtonClass("sm")} mt-3`}
              >
                {copied === "admin"
                  ? t("webAccess.reach.copied")
                  : t("webAccess.reach.copyAdminLink")}
              </button>
            </details>
          ) : (
            <p className="text-xs leading-5 text-amber-700 dark:text-amber-300">
              {t("webAccess.reach.adminAccessMissing")}
            </p>
          )}
        </div>
      ) : null}
    </div>
  );
}
