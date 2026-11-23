import { CircleHelp, Plus } from "lucide-react";
import { useTranslation } from "react-i18next";
import { LcdIcon } from "../../components/Icons";
import { Button } from "../../components/ui/button";
import { LocalSessionsHelp, type LocalSessionsHelpRequest } from "./LocalSessionsHelp";
import type { SessionDialogView } from "./useSessionDialogNavigation";

export function LocalSessionsGuide({
  view,
  showEntryActions,
  request,
  onAddDevice,
  onShowHelp,
  onChooseSessions,
}: {
  view: SessionDialogView;
  showEntryActions: boolean;
  request: LocalSessionsHelpRequest | null;
  onAddDevice: () => void;
  onShowHelp: () => void;
  onChooseSessions: () => void;
}) {
  const { t } = useTranslation("layout");
  return (
    <>
      {view === "sessions" && showEntryActions && (
        <div className="sticky top-0 z-10 -mt-1 flex flex-wrap items-center justify-end gap-2 bg-[var(--color-bg-primary)] py-1">
          <Button
            variant="ghost"
            size="sm"
            className="text-sm max-sm:min-h-11"
            onClick={onShowHelp}
          >
            <LcdIcon icon={CircleHelp} glyph="circle-info" size={16} aria-hidden="true" />
            {t("localSessions.localSetup")}
          </Button>
          <Button
            variant="outline"
            size="sm"
            className="text-sm max-sm:min-h-11"
            onClick={onAddDevice}
          >
            <LcdIcon icon={Plus} glyph="plus" size={16} aria-hidden="true" />
            {t("localSessions.addDevice")}
          </Button>
        </div>
      )}
      <div hidden={view !== "help"}>
        <LocalSessionsHelp request={request} onChooseSessions={onChooseSessions} />
      </div>
    </>
  );
}
