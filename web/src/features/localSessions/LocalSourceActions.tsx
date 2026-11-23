import { useTranslation } from "react-i18next";
import { Button } from "../../components/ui/button";
import type { LocalSessionSource } from "../../types";
import type { useLocalSourceControls } from "./useLocalSourceControls";
import type { useLocalSessionSources } from "./useLocalSessionSources";

export type SourceManager = ReturnType<typeof useLocalSourceControls>;
export type SourceDirectory = ReturnType<typeof useLocalSessionSources>;
export function LocalSourceActions({
  source,
  directory,
  management,
  busy,
}: {
  source: LocalSessionSource;
  directory: SourceDirectory;
  management: SourceManager;
  busy: boolean;
}) {
  const { t } = useTranslation("layout");
  const control = directory.source_states?.[source.source_id]?.control;
  const capable = !!directory.capabilities?.source_control && control?.supported === true;
  const blocked = control?.access_blocked;
  const unavailable = busy || !capable;
  return (
    <div
      className="flex w-full flex-wrap justify-end gap-2 sm:w-auto"
      role="group"
      aria-label={t("sourceControls.actions", { title: source.title })}
    >
      {!blocked && (
        <Button
          size="sm"
          variant="ghost"
          className="min-h-11 text-sm"
          disabled={
            unavailable ||
            !directory.capabilities?.source_diagnostics ||
            !!management.cooldowns[source.source_id]
          }
          onClick={() => void management.recheck(source)}
        >
          {t(
            management.cooldowns[source.source_id]
              ? "sourceControls.checkLater"
              : "sourceControls.recheck",
          )}
        </Button>
      )}
      {blocked && !control.pause_complete && (
        <Button
          size="sm"
          variant="outline"
          className="min-h-11 text-sm"
          disabled={unavailable}
          onClick={(event) => {
            event.currentTarget.focus();
            management.open(source, "remove");
          }}
        >
          {t("sourceControls.finishPause")}
        </Button>
      )}
      <Button
        size="sm"
        variant="outline"
        className="min-h-11 text-sm"
        disabled={unavailable || (!!blocked && !control.pause_complete)}
        onClick={(event) => {
          event.currentTarget.focus();
          management.open(source, blocked ? "restore" : "remove");
        }}
      >
        {t(blocked ? "sourceControls.restore" : "sourceControls.remove")}
      </Button>
    </div>
  );
}
