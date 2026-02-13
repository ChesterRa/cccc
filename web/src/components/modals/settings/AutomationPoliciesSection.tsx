import React from "react";
import { useTranslation } from "react-i18next";

import { BellIcon, NumberInputRow, Section } from "./automationUtils";
import { primaryButtonClass } from "./types";

interface AutomationPoliciesSectionProps {
  isDark: boolean;
  busy: boolean;
  nudgeSeconds: number;
  setNudgeSeconds: (v: number) => void;
  replyRequiredNudgeSeconds: number;
  setReplyRequiredNudgeSeconds: (v: number) => void;
  attentionAckNudgeSeconds: number;
  setAttentionAckNudgeSeconds: (v: number) => void;
  unreadNudgeSeconds: number;
  setUnreadNudgeSeconds: (v: number) => void;
  nudgeDigestMinIntervalSeconds: number;
  setNudgeDigestMinIntervalSeconds: (v: number) => void;
  nudgeMaxRepeatsPerObligation: number;
  setNudgeMaxRepeatsPerObligation: (v: number) => void;
  nudgeEscalateAfterRepeats: number;
  setNudgeEscalateAfterRepeats: (v: number) => void;
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
      <NumberInputRow
        isDark={props.isDark}
        label={t("policies.unreadFollowup")}
        value={props.nudgeSeconds}
        onChange={props.setNudgeSeconds}
        helperText={t("policies.unreadFollowupHelp")}
      />

      <div className="grid grid-cols-1 sm:grid-cols-2 gap-4">
        <NumberInputRow
          isDark={props.isDark}
          label={t("policies.needReplyFollowup")}
          value={props.replyRequiredNudgeSeconds}
          onChange={props.setReplyRequiredNudgeSeconds}
          helperText={t("policies.needReplyFollowupHelp")}
        />
        <NumberInputRow
          isDark={props.isDark}
          label={t("policies.importantFollowup")}
          value={props.attentionAckNudgeSeconds}
          onChange={props.setAttentionAckNudgeSeconds}
          helperText={t("policies.importantFollowupHelp")}
        />
      </div>

      <div className="grid grid-cols-1 sm:grid-cols-2 gap-4">
        <NumberInputRow
          isDark={props.isDark}
          label={t("policies.backlogDigest")}
          value={props.unreadNudgeSeconds}
          onChange={props.setUnreadNudgeSeconds}
          helperText={t("policies.backlogDigestHelp")}
        />
        <NumberInputRow
          isDark={props.isDark}
          label={t("policies.digestMinGap")}
          value={props.nudgeDigestMinIntervalSeconds}
          onChange={props.setNudgeDigestMinIntervalSeconds}
          helperText={t("policies.digestMinGapHelp")}
        />
      </div>

      <div className="grid grid-cols-1 sm:grid-cols-2 gap-4">
        <NumberInputRow
          isDark={props.isDark}
          label={t("policies.maxRepeats")}
          value={props.nudgeMaxRepeatsPerObligation}
          onChange={props.setNudgeMaxRepeatsPerObligation}
          formatValue={false}
          helperText={t("policies.maxRepeatsHelp")}
        />
        <NumberInputRow
          isDark={props.isDark}
          label={t("policies.escalateAfter")}
          value={props.nudgeEscalateAfterRepeats}
          onChange={props.setNudgeEscalateAfterRepeats}
          formatValue={false}
          helperText={t("policies.escalateAfterHelp")}
        />
      </div>

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
          helperText={props.keepaliveMax <= 0 ? t("policies.keepaliveInfinite") : t("policies.keepaliveRetryUp", { count: props.keepaliveMax })}
        />
      </div>

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

      <div className={`pt-2 text-xs font-semibold ${props.isDark ? "text-slate-300" : "text-gray-700"}`}>{t("policies.foremanAlerts")}</div>
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
      <div className="pt-2 flex items-center justify-end">
        <button
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
