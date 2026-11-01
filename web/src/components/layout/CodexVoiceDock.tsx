import { useTranslation } from "react-i18next";
import { VoiceStatusDot } from "../../features/codexVoice/CodexVoiceStatus";
import { voicePhaseHasLocalActivity } from "../../features/codexVoice/codexVoicePhase";
import {
  codexVoiceCallStatus,
  codexVoiceDisplayIdentity,
} from "../../features/codexVoice/codexVoiceControllerText";
import { classNames } from "../../utils/classNames";
import { AlertIcon, StopIcon, VoiceWaveformIcon } from "../Icons";
import { IconButton } from "../ui/icon-button";
import type { CodexVoiceDockProps } from "./codexVoiceDockTypes";

export function CodexVoiceDock({
  controller,
  chetWorking = false,
  collapsed = false,
  variant = "sidebar",
  onOpen,
  onStart,
}: CodexVoiceDockProps) {
  const { t } = useTranslation(["layout", "modals"]);
  const identity = codexVoiceDisplayIdentity(controller);
  const providerLabel = t(`modals:codexVoiceProviders.${identity.provider}`);
  const callStatus = codexVoiceCallStatus(controller);
  const attention = Boolean(
    controller.error ||
    controller.refreshError ||
    controller.playbackBlocked ||
    controller.analyst?.warning ||
    controller.analyst?.phase === "needs_attention",
  );
  const phaseLabel = t(`modals:${callStatus.labelKey}`);
  const working = chetWorking || controller.analystWorking;
  // A call names its own state; otherwise Chet's work, then what needs a look.
  const statusLabel = controller.isEngaged
    ? [providerLabel, identity.voice, phaseLabel].filter(Boolean).join(" · ")
    : controller.refreshError
      ? phaseLabel
      : attention
        ? t("layout:codexVoiceAttention")
        : working
          ? t("modals:chet.state.working")
          : t("modals:chet.subtitle");
  const stopLabel = t(
    controller.externalCall ? "modals:codexVoiceStopExisting" : "modals:codexVoiceStop",
  );
  const startDisabled = controller.checking || controller.isStarting;
  const identityTitle = ["Chet", statusLabel].filter(Boolean).join(" · ");
  const active =
    working ||
    voicePhaseHasLocalActivity(controller.phase, {
      microphoneMuted: controller.microphoneMuted,
      externalCall: controller.externalCall,
      playbackBlocked: controller.playbackBlocked,
    });

  const callControl = controller.isEngaged ? (
    <IconButton
      type="button"
      variant="ghost"
      size="sm"
      onClick={() => void controller.disconnect()}
      disabled={controller.phase === "stopping"}
      className={classNames("flex-none text-rose-500", collapsed && "w-full")}
      data-lcd-key
      label={stopLabel}
    >
      <StopIcon size={16} />
    </IconButton>
  ) : (
    <IconButton
      type="button"
      variant="ghost"
      size="sm"
      onClick={onStart}
      disabled={startDisabled}
      className={classNames("flex-none text-[var(--color-text-secondary)]", collapsed && "w-full")}
      data-lcd-key
      label={t("layout:codexVoiceStart")}
    >
      <VoiceWaveformIcon size={16} />
    </IconButton>
  );

  if (variant === "mobile") {
    return (
      <div className="relative z-10 flex min-h-12 items-center gap-2 border-b border-[var(--glass-border-subtle)] bg-[var(--glass-panel-bg)] px-3 py-2 md:hidden">
        <ConsoleButton
          onOpen={onOpen}
          attention={attention || (controller.isEngaged && callStatus.blocked)}
          active={controller.isEngaged || working}
          working={active}
          status={statusLabel}
          label={t("modals:chet.open")}
        />
        {callControl}
      </div>
    );
  }

  if (collapsed) {
    return (
      <div className="flex shrink-0 flex-col gap-1 border-t border-[var(--glass-border-subtle)] bg-[var(--color-sidebar-bg)] p-2 md:bg-transparent">
        <IconButton
          type="button"
          variant={controller.isEngaged ? "secondary" : "ghost"}
          size="touch"
          className={classNames(
            "relative w-full text-[var(--color-text-secondary)]",
            controller.isEngaged && "text-[var(--color-accent-primary)]",
          )}
          onClick={onOpen}
          label={t("modals:chet.open")}
          title={identityTitle}
        >
          {attention ? <AlertIcon size={18} /> : <ChetMark />}
          <span className="absolute bottom-1.5 right-1.5">
            <VoiceStatusDot
              active={controller.isEngaged || working}
              attention={attention || (controller.isEngaged && callStatus.blocked)}
              working={active}
            />
          </span>
        </IconButton>
        {callControl}
      </div>
    );
  }

  return (
    <div className="shrink-0 border-t border-[var(--glass-border-subtle)] bg-[var(--color-sidebar-bg)] px-2 py-2 md:py-1 md:bg-transparent">
      <div className="flex items-center gap-2">
        <ConsoleButton
          onOpen={onOpen}
          attention={attention || (controller.isEngaged && callStatus.blocked)}
          active={controller.isEngaged || working}
          working={active}
          status={statusLabel}
          label={t("modals:chet.open")}
        />
        {callControl}
      </div>
    </div>
  );
}

/** Chet's mark: its initial, as in the sidebar entry of the design. */
function ChetMark() {
  return (
    <span aria-hidden="true" className="text-sm font-semibold leading-none">
      C
    </span>
  );
}

function ConsoleButton({
  onOpen,
  attention,
  active,
  working,
  status,
  label,
}: {
  onOpen: () => void;
  attention: boolean;
  active: boolean;
  working: boolean;
  status: string;
  label: string;
}) {
  return (
    <button
      type="button"
      onClick={onOpen}
      className="flex min-w-0 flex-1 cursor-pointer items-center gap-3 rounded-xl px-2 py-2 text-left outline-none transition-colors hover:bg-[var(--glass-tab-bg-hover)] focus-visible:ring-2 focus-visible:ring-[var(--color-accent-primary)]"
      aria-label={label}
      title={["Chet", status].join(" · ")}
    >
      <span className="relative flex h-8 w-8 flex-none items-center justify-center rounded-xl bg-[var(--glass-tab-bg)] text-[var(--color-text-secondary)]">
        {attention ? <AlertIcon size={17} /> : <ChetMark />}
        <span className="absolute bottom-0.5 right-0.5">
          <VoiceStatusDot active={active} attention={attention} working={working} />
        </span>
      </span>
      <span className="min-w-0 flex-1">
        <span className="block truncate text-xs font-semibold text-[var(--color-text-primary)]">
          Chet
        </span>
        <span className="mt-0.5 block truncate text-xs text-[var(--color-text-muted)]">
          {status}
        </span>
      </span>
    </button>
  );
}
