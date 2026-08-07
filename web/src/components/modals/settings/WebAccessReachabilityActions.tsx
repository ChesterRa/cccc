import { useTranslation } from "react-i18next";

import {
  isReachabilityActionBlockedByMissingAdminToken,
  isRemoteAccessBlockedByMissingAdminToken,
  type AccessGoal,
  type ReachabilityAction,
} from "./webAccessReachabilityModel";
import { primaryButtonClass, secondaryButtonClass } from "./types";

interface WebAccessReachabilityActionsProps {
  action: ReachabilityAction;
  actionHint: string;
  draftGoal: AccessGoal;
  savedGoal: AccessGoal;
  hasAdminToken: boolean;
  saveBusy: boolean;
  applyBusy: boolean;
  endpoint: string | null;
  onSave: () => void;
  onApply: () => void;
  onCopyEndpoint: () => void | Promise<void>;
}

export function WebAccessReachabilityActions({
  action,
  actionHint,
  draftGoal,
  savedGoal,
  hasAdminToken,
  saveBusy,
  applyBusy,
  endpoint,
  onSave,
  onApply,
  onCopyEndpoint,
}: WebAccessReachabilityActionsProps) {
  const { t } = useTranslation("settings");
  const primaryBlocked = isReachabilityActionBlockedByMissingAdminToken(
    action,
    draftGoal,
    savedGoal,
    hasAdminToken,
  );
  const endpointBlocked = isRemoteAccessBlockedByMissingAdminToken(savedGoal, hasAdminToken);
  const blockedHint = t("webAccess.remoteAdminTokenRequiredHint");
  const primaryBusy = action === "save" ? saveBusy : applyBusy;

  return (
    <div className="mt-4 rounded-xl border border-[var(--glass-border-subtle)] bg-[var(--glass-panel-bg)] p-4">
      <div className="grid gap-4 xl:grid-cols-[minmax(0,1fr)_auto] xl:items-center">
        <div className="min-w-0">
          <div className="text-sm font-medium text-[var(--color-text-primary)]">
            {action === "save"
              ? t("webAccess.saveChanges")
              : action === "apply"
                ? t("webAccess.applyNow")
                : t("webAccess.savedState")}
          </div>
          <div className="mt-1 text-xs leading-6 text-[var(--color-text-muted)]">
            {primaryBlocked ? blockedHint : actionHint}
          </div>
        </div>
        <div className="grid w-full grid-cols-1 gap-2 sm:grid-cols-2 xl:flex xl:w-auto xl:shrink-0 xl:flex-nowrap xl:justify-end">
          <button
            type="button"
            onClick={() => {
              if (primaryBlocked) return;
              if (action === "save") onSave();
              if (action === "apply") onApply();
            }}
            disabled={action === "idle" || saveBusy || applyBusy || primaryBlocked}
            title={primaryBlocked ? blockedHint : undefined}
            className={`${primaryButtonClass(primaryBusy)} w-full xl:w-auto xl:whitespace-nowrap`}
          >
            {action === "save"
              ? saveBusy
                ? t("webAccess.saving")
                : t("webAccess.saveChanges")
              : action === "apply"
                ? applyBusy
                  ? t("common:loading")
                  : t("webAccess.applyNow")
                : t("webAccess.savedState")}
          </button>
          {endpoint ? (
            <button
              type="button"
              onClick={() => {
                if (!endpointBlocked) void onCopyEndpoint();
              }}
              disabled={endpointBlocked}
              title={endpointBlocked ? blockedHint : undefined}
              className={`${secondaryButtonClass()} w-full xl:w-auto xl:whitespace-nowrap`}
            >
              {t("webAccess.copyEndpoint")}
            </button>
          ) : null}
        </div>
      </div>
    </div>
  );
}
