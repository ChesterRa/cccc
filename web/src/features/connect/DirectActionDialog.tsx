import { useState } from "react";
import { useTranslation } from "react-i18next";
import { Dialog, DialogContent, DialogTitle, DialogDescription } from "../../components/ui/dialog";
import { Button } from "../../components/ui/button";

export function DirectActionDialog({
  open,
  busy,
  title,
  description,
  confirm,
  onClose,
  onConfirm,
}: {
  open: boolean;
  busy: boolean;
  title: string;
  description: string;
  confirm: string;
  onClose: () => void;
  onConfirm: () => Promise<boolean>;
}) {
  const { t } = useTranslation("layout");
  const [failed, setFailed] = useState(false);
  return (
    <Dialog
      open={open}
      onOpenChange={(next) => {
        if (!next && !busy) {
          setFailed(false);
          onClose();
        }
      }}
    >
      <DialogContent aria-busy={busy} className="w-[min(calc(100vw-2rem),28rem)] gap-4 p-6">
        <DialogTitle className="pr-8 text-lg font-semibold">{title}</DialogTitle>
        <DialogDescription className="text-sm leading-relaxed">{description}</DialogDescription>
        {failed && (
          <p role="alert" className="text-sm text-[var(--color-danger)]">
            {t("direct.actionFailed")}
          </p>
        )}
        <div className="flex flex-wrap justify-end gap-2">
          <Button
            variant="outline"
            disabled={busy}
            onClick={() => {
              setFailed(false);
              onClose();
            }}
          >
            {t("direct.cancel")}
          </Button>
          <Button
            variant="destructive"
            disabled={busy}
            onClick={async () => {
              setFailed(false);
              if (await onConfirm()) onClose();
              else setFailed(true);
            }}
          >
            {confirm}
          </Button>
        </div>
      </DialogContent>
    </Dialog>
  );
}
