import { useEffect, useRef } from "react";
import { useTranslation } from "react-i18next";
import {
  finishWorkspaceNavigation,
  useWorkspaceNavigation,
} from "../../stores/workspaceNavigation";
import { Button } from "../ui/button";
import { Dialog, DialogContent, DialogDescription, DialogTitle } from "../ui/dialog";

export function WorkspaceNavigationDialog() {
  const { t } = useTranslation("chat");
  const dirty = useWorkspaceNavigation((state) => state.owners.size > 0);
  const pending = useWorkspaceNavigation((state) => state.pending);
  const cancel = useRef<HTMLButtonElement>(null);
  const restoreFocus = useRef<HTMLElement | null>(null);
  if (pending) restoreFocus.current = pending.focus;
  useEffect(() => {
    if (!dirty) return;
    const protect = (event: BeforeUnloadEvent) => {
      event.preventDefault();
      event.returnValue = "";
    };
    window.addEventListener("beforeunload", protect);
    return () => window.removeEventListener("beforeunload", protect);
  }, [dirty]);
  return (
    <Dialog
      open={Boolean(pending)}
      onOpenChange={(open) => !open && finishWorkspaceNavigation(false)}
    >
      <DialogContent
        className="gap-4 p-6"
        onOpenAutoFocus={(event) => {
          event.preventDefault();
          cancel.current?.focus();
        }}
        onCloseAutoFocus={(event) => {
          event.preventDefault();
          // Mobile navigation closes its drawer before this dialog opens. Do
          // not return keyboard focus to a still-mounted, off-screen Group row.
          const target = [
            restoreFocus.current,
            document.querySelector<HTMLElement>("[data-workspace-editor]"),
            document.querySelector<HTMLElement>("[data-sidebar-toggle]"),
            document.querySelector<HTMLElement>("[data-group-title-edit]"),
          ].find((element) => {
            if (!element?.isConnected || element === document.body) return false;
            const rect = element.getBoundingClientRect();
            return (
              rect.width > 0 &&
              rect.height > 0 &&
              rect.right > 0 &&
              rect.bottom > 0 &&
              rect.left < window.innerWidth &&
              rect.top < window.innerHeight
            );
          });
          target?.focus({ preventScroll: true });
        }}
      >
        <DialogTitle className="pr-6">{t("workspaceUnsaved.title")}</DialogTitle>
        <DialogDescription>{t("workspaceUnsaved.description")}</DialogDescription>
        <div className="flex flex-wrap justify-end gap-2">
          <Button ref={cancel} variant="outline" onClick={() => finishWorkspaceNavigation(false)}>
            {t("workspaceUnsaved.stay")}
          </Button>
          <Button onClick={() => finishWorkspaceNavigation(true)}>
            {t("workspaceUnsaved.discard")}
          </Button>
        </div>
      </DialogContent>
    </Dialog>
  );
}
