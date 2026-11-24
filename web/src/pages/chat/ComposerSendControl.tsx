import { useId } from "react";
import { useTranslation } from "react-i18next";
import { ChevronDownIcon, InboxIcon, ReplyIcon, SendIcon, StopIcon } from "../../components/Icons";
import { Button } from "../../components/ui/button";
import { LcdSpinner } from "../../components/LcdSpinner";
import { Popover, PopoverContent, PopoverTrigger } from "../../components/ui/popover";
import type { ComposerMessageMode } from "../../stores/useComposerStore";
import type { ComposerSendKey } from "../../stores/composerSendKey";

export function ComposerSendControl({
  mode,
  options,
  open,
  onOpenChange,
  onModeChange,
  sendKey,
  onSendKeyChange,
  modEnterShortcutLabel,
  onSend,
  onStopUploadWaiting,
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
  sendKey: ComposerSendKey;
  onSendKeyChange: (sendKey: ComposerSendKey) => void;
  /** "⌘+Enter" or "Ctrl+Enter" for this platform. */
  modEnterShortcutLabel: string;
  onSend: () => void;
  onStopUploadWaiting?: () => void;
  busy: boolean;
  canSend: boolean;
  modeDisabled: boolean;
  requestReplyReady: boolean;
  title: string;
}) {
  const { t } = useTranslation("chat");
  const unavailableReasonId = useId();
  const sendKeyLabelId = useId();
  const active = options.find((option) => option.key === mode) || options[0];
  const Icon = mode === "mail" ? InboxIcon : mode === "request_reply" ? ReplyIcon : SendIcon;
  const sendIcon = (
    <Icon
      data-composer-send-icon
      size={14}
      strokeWidth={1.25}
      className={mode === "send" ? undefined : "hidden @min-[600px]/composer:block"}
    />
  );
  const showUnavailableReason =
    !requestReplyReady && options.some((option) => option.key === "request_reply");
  return (
    <div
      data-composer-send-control
      className="ml-auto flex shrink-0 items-stretch rounded-xl bg-[var(--glass-tab-bg)]"
    >
      {onStopUploadWaiting && (
        <Button
          variant="ghost"
          className="h-[44px] min-h-[44px] w-[44px] px-0 @min-[600px]/composer:h-9 @min-[600px]/composer:min-h-9 @min-[600px]/composer:w-9"
          onClick={onStopUploadWaiting}
          aria-label={t("stopUploadWaiting")}
          title={t("stopUploadWaiting")}
        >
          <StopIcon size={14} strokeWidth={1.25} />
        </Button>
      )}
      <Button
        variant="ghost"
        data-composer-send-primary
        data-busy={busy ? "" : undefined}
        className={`h-[44px] min-h-[44px] min-w-[44px] rounded-l-xl rounded-r-none text-xs shadow-none @min-[600px]/composer:h-9 @min-[600px]/composer:min-h-9 @min-[600px]/composer:min-w-11 @min-[600px]/composer:px-3 ${
          mode === "send"
            ? "px-3"
            : mode === "request_reply"
              ? "w-[64px] px-0 @min-[360px]/composer:w-auto @min-[360px]/composer:px-2"
              : "px-2"
        }`}
        onClick={onSend}
        disabled={busy || !canSend}
        // The key says at once that the message is being sent; its mark follows later.
        aria-label={
          busy
            ? t("sending")
            : mode === "send"
              ? t("sendMessage")
              : t("messageMode", { mode: active.label })
        }
        title={busy ? t("sending") : title}
      >
        {busy ? <LcdSpinner className="h-4 w-4">{sendIcon}</LcdSpinner> : sendIcon}
        <span
          data-composer-send-label
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
          {/* Which key sends, as a setting rather than a fourth way to send; this browser
              remembers the choice. The hint says how to start a new line with it. */}
          <div className="mt-1.5 border-t border-[var(--glass-border-subtle)] px-3 pt-2.5 pb-1.5">
            <div className="flex items-center justify-between gap-3">
              <span
                id={sendKeyLabelId}
                className="min-w-0 whitespace-nowrap text-xs text-[var(--color-text-secondary)]"
              >
                {t("sendKeyOptions")}
              </span>
              <div
                role="group"
                aria-labelledby={sendKeyLabelId}
                data-composer-send-key
                className="grid shrink-0 grid-cols-2 rounded-[10px] bg-[var(--glass-tab-bg)] p-[3px]"
              >
                {(
                  [
                    ["enter", "Enter", t("sendKeyEnter")],
                    [
                      "mod_enter",
                      modEnterShortcutLabel,
                      t("sendKeyModEnter", { shortcut: modEnterShortcutLabel }),
                    ],
                  ] as const
                ).map(([key, label, description]) => (
                  <button
                    key={key}
                    type="button"
                    aria-pressed={sendKey === key}
                    aria-label={description}
                    title={description}
                    onClick={() => onSendKeyChange(key)}
                    className={`h-7 min-w-0 whitespace-nowrap rounded-lg px-2 text-xs transition-colors focus-visible:outline-2 ${
                      sendKey === key
                        ? "bg-[var(--color-bg-primary)] font-semibold text-[var(--color-text-primary)] shadow-[0_1px_2px_rgba(24,24,27,0.08)]"
                        : "text-[var(--color-text-tertiary)] hover:text-[var(--color-text-primary)]"
                    }`}
                  >
                    {label}
                  </button>
                ))}
              </div>
            </div>
            <p
              data-composer-send-key-hint
              className="mt-1.5 text-xs text-[var(--color-text-muted)]"
            >
              {sendKey === "enter" ? t("sendKeyEnterHint") : t("sendKeyModEnterHint")}
            </p>
          </div>
        </PopoverContent>
      </Popover>
    </div>
  );
}
