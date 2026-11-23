import { useRef } from "react";
import { useTranslation } from "react-i18next";
import { Dialog, DialogContent, DialogTitle, DialogDescription } from "../../components/ui/dialog";
import { Button } from "../../components/ui/button";
import type { SourceManager } from "./LocalSourceActions";

export function LocalSourceConfirmation({ management }: { management: SourceManager }) {
  const { t } = useTranslation("layout");
  const previous = useRef<Element | null>(document.activeElement);
  const heading = useRef<HTMLHeadingElement>(null);
  const target = management.target;
  const blocked = management.targetState?.control.access_blocked;
  const partial = blocked && !management.targetState?.control.pause_complete;
  return (
    <Dialog
      open={!!target}
      onOpenChange={(open) => {
        if (!open) management.close();
      }}
    >
      <DialogContent
        aria-busy={management.busy}
        className="w-[min(calc(100vw-2rem),32rem)] max-h-[90dvh] gap-4 overflow-y-auto rounded-none p-5 font-mono"
        onOpenAutoFocus={(event) => {
          previous.current = document.activeElement;
          event.preventDefault();
          heading.current?.focus({ preventScroll: true });
        }}
        onCloseAutoFocus={(event) => {
          event.preventDefault();
          const opener = previous.current;
          if (
            opener instanceof HTMLElement &&
            opener !== document.body &&
            opener.isConnected &&
            !opener.matches(":disabled")
          )
            opener.focus();
          if (document.activeElement === document.body || !opener?.isConnected)
            // The opener row can disappear after a removal; return to the section that lists sources.
            document.querySelector<HTMLElement>("[data-source-control-return]")?.focus();
        }}
        onEscapeKeyDown={(event) => {
          if (management.busy) event.preventDefault();
        }}
        onPointerDownOutside={(event) => {
          if (management.busy) event.preventDefault();
        }}
      >
        <DialogTitle
          ref={heading}
          tabIndex={-1}
          className="break-words pr-8 text-base [overflow-wrap:anywhere]"
        >
          {t(target?.action === "restore" ? "sourceControls.restore" : "sourceControls.remove")} ·{" "}
          {target?.source.title}
        </DialogTitle>
        <DialogDescription className="break-words text-sm leading-relaxed">
          {t(
            target?.action === "restore"
              ? "sourceControls.restoreConfirm"
              : "sourceControls.removeConfirm",
          )}
        </DialogDescription>
        {(blocked || management.notice) && (
          <p role="status" className="break-words text-sm">
            {blocked
              ? t(partial ? "sourceControls.partial" : "sourceControls.blocked")
              : management.notice}
          </p>
        )}
        {management.error && (
          <p role="alert" className="break-words text-sm text-[var(--color-accent-danger)]">
            {management.error}
          </p>
        )}
        <div className="flex flex-wrap justify-end gap-2">
          <Button
            variant="outline"
            className="min-h-11"
            disabled={management.busy}
            onClick={management.close}
          >
            {t("common:cancel")}
          </Button>
          <Button
            variant={target?.action === "restore" ? "default" : "destructive"}
            className="min-h-11"
            disabled={management.busy || (target?.action === "restore" && !!partial)}
            onClick={() => void management.confirm()}
          >
            {t(
              management.busy
                ? "sourceControls.working"
                : target?.action === "restore"
                  ? "sourceControls.confirmRestore"
                  : partial
                    ? "sourceControls.finishPause"
                    : "sourceControls.confirmRemove",
            )}
          </Button>
        </div>
      </DialogContent>
    </Dialog>
  );
}
