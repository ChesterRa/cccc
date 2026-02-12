import React from "react";

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
  return (
    <Section
      isDark={props.isDark}
      icon={BellIcon}
      title="Engine Policies"
      description="Built-in follow-ups and alerts. Adjust values, then click Save Policies."
    >
      <NumberInputRow
        isDark={props.isDark}
        label="Unread Follow-up (sec)"
        value={props.nudgeSeconds}
        onChange={props.setNudgeSeconds}
        helperText="Remind a member when unread messages sit too long."
      />

      <div className="grid grid-cols-1 sm:grid-cols-2 gap-4">
        <NumberInputRow
          isDark={props.isDark}
          label="Need Reply Follow-up (sec)"
          value={props.replyRequiredNudgeSeconds}
          onChange={props.setReplyRequiredNudgeSeconds}
          helperText="For messages marked Need Reply."
        />
        <NumberInputRow
          isDark={props.isDark}
          label="Important Follow-up (sec)"
          value={props.attentionAckNudgeSeconds}
          onChange={props.setAttentionAckNudgeSeconds}
          helperText="For important messages awaiting acknowledgement."
        />
      </div>

      <div className="grid grid-cols-1 sm:grid-cols-2 gap-4">
        <NumberInputRow
          isDark={props.isDark}
          label="Backlog Digest Follow-up (sec)"
          value={props.unreadNudgeSeconds}
          onChange={props.setUnreadNudgeSeconds}
          helperText="For regular unread backlog digests."
        />
        <NumberInputRow
          isDark={props.isDark}
          label="Digest Minimum Gap (sec)"
          value={props.nudgeDigestMinIntervalSeconds}
          onChange={props.setNudgeDigestMinIntervalSeconds}
          helperText="Minimum gap between digests for the same member."
        />
      </div>

      <div className="grid grid-cols-1 sm:grid-cols-2 gap-4">
        <NumberInputRow
          isDark={props.isDark}
          label="Max Repeats Per Item"
          value={props.nudgeMaxRepeatsPerObligation}
          onChange={props.setNudgeMaxRepeatsPerObligation}
          formatValue={false}
          helperText="Maximum follow-ups for one pending item."
        />
        <NumberInputRow
          isDark={props.isDark}
          label="Escalate To Foreman After"
          value={props.nudgeEscalateAfterRepeats}
          onChange={props.setNudgeEscalateAfterRepeats}
          formatValue={false}
          helperText="Escalate when repeat count reaches this value."
        />
      </div>

      <div className="grid grid-cols-1 sm:grid-cols-2 gap-4">
        <NumberInputRow
          isDark={props.isDark}
          label="Keepalive Delay (sec)"
          value={props.keepaliveSeconds}
          onChange={props.setKeepaliveSeconds}
          helperText="Wait time after an actor says 'Next:'."
        />
        <NumberInputRow
          isDark={props.isDark}
          label="Keepalive Max Retries"
          value={props.keepaliveMax}
          onChange={props.setKeepaliveMax}
          formatValue={false}
          helperText={props.keepaliveMax <= 0 ? "Infinite retries" : `Retry up to ${props.keepaliveMax} times`}
        />
      </div>

      <div className="grid grid-cols-1 sm:grid-cols-2 gap-4">
        <NumberInputRow
          isDark={props.isDark}
          label="Help Refresh Interval (sec)"
          value={props.helpNudgeIntervalSeconds}
          onChange={props.setHelpNudgeIntervalSeconds}
          helperText="Time since last help follow-up."
        />
        <NumberInputRow
          isDark={props.isDark}
          label="Help Refresh Min Msgs"
          value={props.helpNudgeMinMessages}
          onChange={props.setHelpNudgeMinMessages}
          formatValue={false}
          helperText="Minimum accumulated messages."
        />
      </div>

      <div className={`pt-2 text-xs font-semibold ${props.isDark ? "text-slate-300" : "text-gray-700"}`}>Foreman Alerts</div>
      <NumberInputRow
        isDark={props.isDark}
        label="Actor Idle Alert (sec)"
        value={props.idleSeconds}
        onChange={props.setIdleSeconds}
        helperText="Alert foreman if actor is inactive for this long."
      />

      <NumberInputRow
        isDark={props.isDark}
        label="Group Silence Check (sec)"
        value={props.silenceSeconds}
        onChange={props.setSilenceSeconds}
        helperText="Alert foreman if the entire group is silent."
      />
      <div className="pt-2 flex items-center justify-end">
        <button
          onClick={props.onSavePolicies}
          disabled={props.busy}
          className={`${primaryButtonClass(props.busy)} w-full sm:w-auto`}
          title="Save engine policy settings"
        >
          {props.busy ? "Saving..." : "Save Policies"}
        </button>
      </div>
    </Section>
  );
}
