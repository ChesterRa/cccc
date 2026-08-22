import React from "react";
import { useTranslation } from "react-i18next";

import { BellIcon, NumberInputRow, Section } from "./automationUtils";
import { primaryButtonClass, secondaryButtonClass } from "./types";

interface AutomationPoliciesSectionProps {
  isDark: boolean;
  busy: boolean;
  keepaliveSeconds: number;
  setKeepaliveSeconds: (v: number) => void;
  keepaliveMax: number;
  setKeepaliveMax: (v: number) => void;
  helpNudgeIntervalSeconds: number;
  setHelpNudgeIntervalSeconds: (v: number) => void;
  helpNudgeMinMessages: number;
  setHelpNudgeMinMessages: (v: number) => void;
  idleSeconds: number;
  setIdleSeconds: (v: number) => void;
  silenceSeconds: number;
  setSilenceSeconds: (v: number) => void;
  onSavePolicies: () => void;
  onResetPolicies: () => void;
}

function PolicyGroup({
  title,
  description,
  children,
}: {
  title: string;
  description?: string;
  children: React.ReactNode;
}) {
  return (
    <div className="rounded-xl border border-[var(--glass-border-subtle)] bg-[var(--glass-panel-bg)] p-3.5 space-y-3">
      <div>
        <div className="text-xs font-semibold tracking-[0.02em] text-[var(--color-text-secondary)]">
          {title}
        </div>
        {description ? (
          <div className="mt-1 text-[11px] leading-snug text-[var(--color-text-muted)]">
            {description}
          </div>
        ) : null}
      </div>
      {children}
    </div>
  );
}

export function AutomationPoliciesSection(props: AutomationPoliciesSectionProps) {
  const { t } = useTranslation("settings");
  return (
    <Section
      isDark={props.isDark}
      icon={BellIcon}
      title={t("policies.title")}
      description={t("policies.description")}
    >
      <PolicyGroup
        title={t("policies.progressFollowups")}
        description={t("policies.progressFollowupsHelp")}
      >
        <div className="grid grid-cols-1 sm:grid-cols-2 gap-4">
          <NumberInputRow
            isDark={props.isDark}
            label={t("policies.keepaliveDelay")}
            value={props.keepaliveSeconds}
            onChange={props.setKeepaliveSeconds}
            helperText={t("policies.keepaliveDelayHelp")}
          />
          <NumberInputRow
            isDark={props.isDark}
            label={t("policies.keepaliveMaxRetries")}
            value={props.keepaliveMax}
            onChange={props.setKeepaliveMax}
            formatValue={false}
            helperText={
              props.keepaliveMax <= 0
                ? t("policies.keepaliveOff")
                : t("policies.keepaliveRetryUp", { count: props.keepaliveMax })
            }
          />
        </div>
      </PolicyGroup>

      <PolicyGroup
        title={t("policies.contextRefresh")}
        description={t("policies.contextRefreshHelp")}
      >
        <div className="grid grid-cols-1 sm:grid-cols-2 gap-4">
          <NumberInputRow
            isDark={props.isDark}
            label={t("policies.helpRefreshInterval")}
            value={props.helpNudgeIntervalSeconds}
            onChange={props.setHelpNudgeIntervalSeconds}
            helperText={t("policies.helpRefreshIntervalHelp")}
          />
          <NumberInputRow
            isDark={props.isDark}
            label={t("policies.helpRefreshMinMsgs")}
            value={props.helpNudgeMinMessages}
            onChange={props.setHelpNudgeMinMessages}
            formatValue={false}
            helperText={t("policies.helpRefreshMinMsgsHelp")}
          />
        </div>
      </PolicyGroup>

      <PolicyGroup
        title={t("policies.foremanAlerts")}
        description={t("policies.foremanAlertsHelp")}
      >
        <div className="grid grid-cols-1 sm:grid-cols-2 gap-4">
          <NumberInputRow
            isDark={props.isDark}
            label={t("policies.actorIdleAlert")}
            value={props.idleSeconds}
            onChange={props.setIdleSeconds}
            helperText={t("policies.actorIdleAlertHelp")}
          />
          <NumberInputRow
            isDark={props.isDark}
            label={t("policies.groupSilenceCheck")}
            value={props.silenceSeconds}
            onChange={props.setSilenceSeconds}
            helperText={t("policies.groupSilenceCheckHelp")}
          />
        </div>
      </PolicyGroup>

      <div className="pt-2 flex flex-col sm:flex-row sm:items-center sm:justify-end gap-2">
        <button
          type="button"
          onClick={props.onResetPolicies}
          disabled={props.busy}
          className={`${secondaryButtonClass("md")} w-full sm:w-auto whitespace-nowrap`}
          title={t("policies.resetPoliciesTitle")}
        >
          {t("policies.resetPolicies")}
        </button>
        <button
          type="button"
          onClick={props.onSavePolicies}
          disabled={props.busy}
          className={`${primaryButtonClass(props.busy)} w-full sm:w-auto whitespace-nowrap`}
          title={t("policies.savePoliciesTitle")}
        >
          {props.busy ? t("automation.saving") : t("policies.savePolicies")}
        </button>
      </div>
    </Section>
  );
}
