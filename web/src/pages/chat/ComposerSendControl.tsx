import { useId } from "react";
import { useTranslation } from "react-i18next";
import { ChevronDownIcon, InboxIcon, ReplyIcon, SendIcon } from "../../components/Icons";
import { Button } from "../../components/ui/button";
import { Popover, PopoverContent, PopoverTrigger } from "../../components/ui/popover";
import type { ComposerMessageMode } from "../../stores/useComposerStore";

export function ComposerSendControl({
  mode,
  options,
  open,
  onOpenChange,
  onModeChange,
  onSend,
  busy,
  canSend,
  modeDisabled,
  requestReplyReady,
  title,
}: {
  mode: ComposerMessageMode;
  options: { key: ComposerMessageMode; label: string; description: string }[];
  open: boolean;
  onOpenChange: (open: boolean) => void;
  onModeChange: (mode: ComposerMessageMode) => void;
  onSend: () => void;
  busy: boolean;
  canSend: boolean;
  modeDisabled: boolean;
  requestReplyReady: boolean;
  title: string;
}) {
  const { t } = useTranslation("chat");
  const unavailableReasonId = useId();
  const active = options.find((option) => option.key === mode) || options[0];
  const Icon = mode === "mail" ? InboxIcon : mode === "request_reply" ? ReplyIcon : SendIcon;
  const showUnavailableReason =
    !requestReplyReady && options.some((option) => option.key === "request_reply");
  return (
    <div
      data-composer-send-control
      className="ml-auto flex shrink-0 items-stretch rounded-xl bg-[var(--glass-tab-bg)]"
    >
      <Button
        variant="ghost"
        className={`h-[44px] min-h-[44px] min-w-[44px] rounded-l-xl rounded-r-none text-xs shadow-none @min-[600px]/composer:h-9 @min-[600px]/composer:min-h-9 @min-[600px]/composer:min-w-11 @min-[600px]/composer:px-3 ${
          mode === "send"
            ? "px-3"
            : mode === "request_reply"
              ? "w-[64px] px-0 @min-[360px]/composer:w-auto @min-[360px]/composer:px-2"
              : "px-2"
        }`}
        onClick={onSend}
        disabled={busy || !canSend}
        aria-label={mode === "send" ? t("sendMessage") : t("messageMode", { mode: active.label })}
        title={title}
      >
        {busy ? (
          <span className="h-4 w-4 animate-spin rounded-full border-2 border-current border-t-transparent" />
        ) : (
          <Icon
            size={14}
            strokeWidth={1.25}
            className={mode === "send" ? undefined : "hidden @min-[600px]/composer:block"}
          />
        )}
        <span
          className={
            mode === "send"
              ? "hidden max-w-28 truncate @min-[600px]/composer:inline"
              : mode === "request_reply"
                ? "max-w-28 whitespace-normal text-center leading-tight @min-[360px]/composer:truncate"
                : "max-w-28 truncate"
          }
        >
          {active.label}
        </span>
      </Button>
      <Popover open={open} onOpenChange={onOpenChange}>
        <PopoverTrigger asChild>
          <Button
            variant="ghost"
            data-composer-send-options
            className="h-[44px] min-h-[44px] w-[44px] rounded-l-none rounded-r-xl border-0 px-0 @min-[600px]/composer:h-9 @min-[600px]/composer:min-h-9 @min-[600px]/composer:w-8"
            disabled={modeDisabled}
            aria-label={t("messageTypeOptions")}
            title={t("messageMode", { mode: active.label })}
          >
            <ChevronDownIcon size={13} strokeWidth={1.25} />
          </Button>
        </PopoverTrigger>
        <PopoverContent
          side="top"
          align="end"
          className="w-64 max-w-[calc(100vw-2rem)] p-1.5"
          aria-label={t("messageTypeOptions")}
        >
          <div
            role="group"
            aria-label={t("messageTypeOptions")}
            aria-describedby={showUnavailableReason ? unavailableReasonId : undefined}
          >
            {options.map((option) => {
              const unavailable = option.key === "request_reply" && !requestReplyReady;
              return (
                <div key={option.key}>
                  <Button
                    variant="ghost"
                    aria-pressed={mode === option.key}
                    aria-describedby={unavailable ? unavailableReasonId : undefined}
                    disabled={unavailable}
                    title={unavailable ? t("modeSendReplyRequiresConcrete") : option.description}
                    className="w-full justify-between whitespace-normal text-left"
                    onClick={() => {
                      onModeChange(option.key);
                      onOpenChange(false);
                    }}
                  >
                    {option.label}
                    {mode === option.key && <span aria-hidden="true">✓</span>}
                  </Button>
                  {unavailable ? (
                    <p
                      id={unavailableReasonId}
                      className="px-3 pb-2 text-xs text-[var(--color-text-secondary)]"
                    >
                      {t("modeSendReplyRequiresConcrete")}
                    </p>
                  ) : null}
                </div>
              );
            })}
          </div>
        </PopoverContent>
      </Popover>
    </div>
  );
}
