import { useTranslation } from "react-i18next";
import { VoiceStatusDot } from "../../features/codexVoice/CodexVoiceStatus";
import { classNames } from "../../utils/classNames";
import { AlertIcon, HeadphonesIcon, StopIcon, VoiceWaveformIcon } from "../Icons";
import { IconButton } from "../ui/icon-button";
import type { CodexVoiceDockProps } from "./codexVoiceDockTypes";

export function CodexVoiceDock({
  controller,
  selectedGroupId,
  collapsed = false,
  variant = "sidebar",
  onOpen,
  onStart,
}: CodexVoiceDockProps) {
  const { t } = useTranslation(["layout", "modals"]);
  const attention = Boolean(
    controller.error ||
    controller.playbackBlocked ||
    controller.analyst?.warning ||
    controller.analyst?.phase === "needs_attention",
  );
  const phaseLabel = controller.externalCall
    ? t("modals:codexVoiceActiveElsewhere")
    : controller.checking && !controller.isEngaged
      ? t("modals:codexVoiceChecking")
      : t(`modals:codexVoicePhase.${controller.phase}`);
  const statusLabel = attention
    ? t("layout:codexVoiceAttention")
    : controller.analyst && !controller.isEngaged
      ? t("layout:codexVoiceAnalystReady")
      : phaseLabel;
  const stopLabel = t(
    controller.externalCall ? "modals:codexVoiceStopExisting" : "modals:codexVoiceStop",
  );
  const startDisabled =
    (!selectedGroupId && !controller.analyst) || controller.checking || controller.isStarting;

  const callControl = controller.isEngaged ? (
    <IconButton
      type="button"
      variant="ghost"
      size="sm"
      onClick={() => void controller.disconnect()}
      disabled={controller.phase === "stopping"}
      className={classNames("flex-none text-rose-500", collapsed && "w-full")}
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
          attention={attention}
          active={controller.isEngaged}
          status={statusLabel}
          label={t("layout:codexVoiceOpenConsole")}
        />
        {callControl}
      </div>
    );
  }

  if (collapsed) {
    return (
      <div className="flex flex-col gap-1 border-t border-[var(--glass-border-subtle)] bg-[var(--color-sidebar-bg)] p-2 md:bg-transparent">
        <IconButton
          type="button"
          variant={controller.isEngaged ? "secondary" : "ghost"}
          size="touch"
          className={classNames(
            "relative w-full text-[var(--color-text-secondary)]",
            controller.isEngaged && "text-[var(--color-accent-primary)]",
          )}
          onClick={onOpen}
          label={t("layout:codexVoiceOpenConsole")}
        >
          {attention ? <AlertIcon size={18} /> : <HeadphonesIcon size={18} />}
          <span className="absolute bottom-1.5 right-1.5">
            <VoiceStatusDot active={controller.isEngaged} attention={attention} />
          </span>
        </IconButton>
        {callControl}
      </div>
    );
  }

  return (
    <div className="border-t border-[var(--glass-border-subtle)] bg-[var(--color-sidebar-bg)] px-3 py-3 md:bg-transparent">
      <div className="flex items-center gap-2">
        <ConsoleButton
          onOpen={onOpen}
          attention={attention}
          active={controller.isEngaged}
          status={statusLabel}
          label={t("layout:codexVoiceOpenConsole")}
        />
        {callControl}
      </div>
    </div>
  );
}

function ConsoleButton({
  onOpen,
  attention,
  active,
  status,
  label,
}: {
  onOpen: () => void;
  attention: boolean;
  active: boolean;
  status: string;
  label: string;
}) {
  const { t } = useTranslation("layout");
  return (
    <button
      type="button"
      onClick={onOpen}
      className="flex min-w-0 flex-1 cursor-pointer items-center gap-3 rounded-xl px-2 py-2 text-left outline-none transition-colors hover:bg-[var(--glass-tab-bg-hover)] focus-visible:ring-2 focus-visible:ring-[var(--color-accent-primary)]"
      aria-label={label}
    >
      <span className="relative flex h-8 w-8 flex-none items-center justify-center rounded-xl bg-[var(--glass-tab-bg)] text-[var(--color-text-secondary)]">
        {attention ? <AlertIcon size={17} /> : <HeadphonesIcon size={17} />}
        <span className="absolute bottom-0.5 right-0.5">
          <VoiceStatusDot active={active} attention={attention} />
        </span>
      </span>
      <span className="min-w-0 flex-1">
        <span className="block truncate text-xs font-semibold text-[var(--color-text-primary)]">
          {t("codexVoice")}
        </span>
        <span className="mt-0.5 block truncate text-[11px] text-[var(--color-text-muted)]">
          {status}
        </span>
      </span>
    </button>
  );
}
