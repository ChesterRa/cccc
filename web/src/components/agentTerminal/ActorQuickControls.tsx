import { useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import { Maximize2, MoreHorizontal, Play, RefreshCw, MousePointer2 } from "lucide-react";
import { Popover, PopoverContent, PopoverTrigger } from "../ui/popover";

export function ActorQuickControls({
  actorTitle,
  running,
  busy,
  readOnly,
  hasTerminal,
  writable,
  connected,
  connectionFailed,
  canStartNewSession,
  unreadCount,
  onInterrupt,
  onLaunch,
  onReconnect,
  onTakeover,
  onHistory,
  onNewSession,
  onRestart,
  onStop,
  onEdit,
  onInbox,
  onRemove,
  onExpand,
}: {
  actorTitle: string;
  running: boolean;
  busy: boolean;
  readOnly: boolean;
  hasTerminal: boolean;
  writable: boolean;
  connected: boolean;
  connectionFailed: boolean;
  canStartNewSession: boolean;
  unreadCount: number;
  onInterrupt: () => void;
  onLaunch: () => void;
  onReconnect: () => void;
  onTakeover: () => void;
  onHistory: () => void;
  onNewSession: () => void;
  onRestart: () => void;
  onStop: () => void;
  onEdit: () => void;
  onInbox: () => void;
  onRemove: () => void;
  onExpand?: () => void;
}) {
  const { t } = useTranslation("actors");
  const [open, setOpen] = useState(false);
  const openingDialog = useRef(false);
  const triggerRef = useRef<HTMLButtonElement>(null);
  const button =
    "flex h-7 w-7 shrink-0 items-center justify-center rounded-md text-[var(--color-text-tertiary)] hover:bg-[var(--glass-tab-bg)] hover:text-[var(--color-text-primary)] disabled:opacity-35 focus-visible:outline-2";
  const item =
    "flex min-h-8 w-full items-center rounded-md px-2 py-1.5 text-left text-xs hover:bg-[var(--glass-tab-bg)] disabled:opacity-35 focus-visible:outline-2";
  const run = (action: () => void, dialog = false) => {
    openingDialog.current = dialog;
    // Give the dialog a stable return target before this menu unmounts.
    if (dialog) triggerRef.current?.focus();
    setOpen(false);
    action();
  };
  return (
    <div
      className="ml-auto flex shrink-0 items-center @min-[480px]/actor-view:gap-0.5"
      data-actor-quick-controls
    >
      {!readOnly && !running ? (
        <button
          className={button}
          disabled={busy}
          onClick={onLaunch}
          title={t("launchAgentLabel")}
          aria-label={t("launchAgentLabel")}
        >
          <Play size={14} />
        </button>
      ) : null}
      {!readOnly && running && hasTerminal ? (
        connectionFailed ? (
          <button
            className={button}
            onClick={onReconnect}
            title={t("reconnect")}
            aria-label={t("reconnect")}
          >
            <RefreshCw size={14} />
          </button>
        ) : connected && !writable ? (
          <button
            className={`${button} @min-[480px]/actor-view:w-auto @min-[480px]/actor-view:px-1 text-[10px]`}
            onClick={onTakeover}
            title={t("takeControl")}
            aria-label={t("takeControl")}
          >
            <MousePointer2 size={14} />
            <span className="hidden @min-[480px]/actor-view:inline">{t("takeControl")}</span>
          </button>
        ) : (
          <button
            className={`${button} hidden @min-[480px]/actor-view:inline-flex`}
            disabled={!connected || !writable || busy}
            onClick={onInterrupt}
            title={t("sendInterruptTitle")}
            aria-label={t("sendInterruptLabel")}
          >
            ⌃C
          </button>
        )
      ) : null}
      {!readOnly || hasTerminal ? (
        <Popover
          open={open}
          onOpenChange={(value) => {
            openingDialog.current = false;
            setOpen(value);
          }}
        >
          <PopoverTrigger asChild>
            <button
              ref={triggerRef}
              className={`${button} relative`}
              title={t("actorControls", { actor: actorTitle })}
              aria-label={t("actorControls", { actor: actorTitle })}
            >
              <MoreHorizontal size={15} />
              {unreadCount > 0 ? (
                <span
                  className="absolute right-0 top-0 h-1.5 w-1.5 rounded-full bg-amber-500"
                  aria-label={t("unreadMessages", { count: unreadCount })}
                />
              ) : null}
            </button>
          </PopoverTrigger>
          <PopoverContent
            align="end"
            sideOffset={4}
            className="w-48 max-h-[min(440px,70vh)] overflow-y-auto rounded-xl p-1.5"
            aria-label={t("actorControls", { actor: actorTitle })}
            onEscapeKeyDown={(event) => event.stopPropagation()}
            onKeyDown={(event) => {
              if (event.key === "Tab") event.stopPropagation();
            }}
            onCloseAutoFocus={(event) => {
              if (openingDialog.current) event.preventDefault();
            }}
          >
            <div className="truncate border-b border-[var(--glass-border-subtle)] px-2 py-1.5 text-xs font-semibold">
              {actorTitle}
            </div>
            {hasTerminal ? (
              <button className={item} onClick={() => run(onHistory, true)}>
                {t("terminalHistory")}
              </button>
            ) : null}
            {!readOnly ? (
              <>
                {running && hasTerminal ? (
                  <button
                    className={item}
                    disabled={!connected || !writable || busy}
                    onClick={() => run(onInterrupt)}
                  >
                    {t("sendInterruptLabel")}
                  </button>
                ) : null}
                {!running ? (
                  <button className={item} disabled={busy} onClick={() => run(onLaunch)}>
                    {t("launchAgentLabel")}
                  </button>
                ) : null}
                {canStartNewSession ? (
                  <button className={item} disabled={busy} onClick={() => run(onNewSession)}>
                    {t("newSession")}
                  </button>
                ) : null}
                {running ? (
                  <button className={item} disabled={busy} onClick={() => run(onRestart)}>
                    {t("relaunch")}
                  </button>
                ) : null}
                <button className={item} disabled={busy} onClick={() => run(onEdit, true)}>
                  {t("editAgentConfig")}
                </button>
                <button className={item} onClick={() => run(onInbox, true)}>
                  {t("inbox")}
                  {unreadCount > 0 ? ` · ${unreadCount > 99 ? "99+" : unreadCount}` : ""}
                </button>
                <div className="my-1 border-t border-[var(--glass-border-subtle)]" />
                {running ? (
                  <button className={item} disabled={busy} onClick={() => run(onStop)}>
                    {t("quitAgent")}
                  </button>
                ) : null}
                <button
                  className={`${item} text-rose-600 dark:text-rose-400`}
                  disabled={busy || running}
                  onClick={() => run(onRemove)}
                >
                  {t("removeAgent")}
                </button>
              </>
            ) : null}
          </PopoverContent>
        </Popover>
      ) : null}
      <button
        className={button}
        onClick={onExpand}
        title={t("chat:workView.maximize", { actor: actorTitle })}
        aria-label={t("chat:workView.maximize", { actor: actorTitle })}
      >
        <Maximize2 size={14} />
      </button>
    </div>
  );
}
