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
  const active = options.find((option) => option.key === mode) || options[0];
  const Icon = mode === "mail" ? InboxIcon : mode === "request_reply" ? ReplyIcon : SendIcon;
  return (
    <div
      data-composer-send-control
      className="ml-auto flex shrink-0 items-stretch rounded-xl bg-[var(--glass-tab-bg)]"
    >
      <Button
        variant="ghost"
        className="h-11 min-h-11 min-w-11 rounded-l-xl rounded-r-none px-3 text-xs shadow-none sm:h-9 sm:min-h-9"
        onClick={onSend}
        disabled={busy || !canSend}
        aria-label={mode === "send" ? t("sendMessage") : t("messageMode", { mode: active.label })}
        title={title}
      >
        {busy ? (
          <span className="h-4 w-4 animate-spin rounded-full border-2 border-current border-t-transparent" />
        ) : (
          <Icon size={14} strokeWidth={1.25} />
        )}
        <span className="hidden max-w-28 truncate sm:inline">{active.label}</span>
      </Button>
      <Popover open={open} onOpenChange={onOpenChange}>
        <PopoverTrigger asChild>
          <Button
            variant="ghost"
            className="h-11 min-h-11 w-11 rounded-l-none rounded-r-xl border-0 px-0 sm:h-9 sm:min-h-9 sm:w-8"
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
          <div role="group" aria-label={t("messageTypeOptions")}>
            {options.map((option) => {
              const unavailable = option.key === "request_reply" && !requestReplyReady;
              return (
                <Button
                  key={option.key}
                  variant="ghost"
                  aria-pressed={mode === option.key}
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
              );
            })}
          </div>
        </PopoverContent>
      </Popover>
    </div>
  );
}
