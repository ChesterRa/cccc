import { useState } from "react";
import { createPortal } from "react-dom";
import { useTranslation } from "react-i18next";
import { Button } from "../../components/ui/button";
import { LocalSessionsIcon } from "../../components/Icons";
import { IconButton } from "../../components/ui/icon-button";
import { LocalSessionsDialog } from "./LocalSessionsDialog";

export function LocalSessionsButton({
  collapsed,
  isDark,
  onSelectGroup,
}: {
  collapsed: boolean;
  isDark: boolean;
  onSelectGroup: (id: string) => void;
}) {
  const { t } = useTranslation("layout");
  const [open, setOpen] = useState(false);
  // A fixed key beside the Group list heading, so it neither scrolls away with many
  // Groups nor reads as one of them; collapsed, it sits under "new" as an icon.
  return (
    <>
      {collapsed ? (
        <IconButton
          type="button"
          size="touch"
          onClick={() => setOpen(true)}
          label={t("localSessions.connect")}
        >
          <LocalSessionsIcon size={18} />
        </IconButton>
      ) : (
        <Button
          type="button"
          variant="ghost"
          size="sm"
          className="h-[44px] min-h-[44px] shrink-0 gap-1 px-1 py-0 text-[0.8125rem] md:h-8 md:min-h-8"
          onClick={() => setOpen(true)}
          title={t("localSessions.connect")}
        >
          <LocalSessionsIcon size={14} aria-hidden="true" />
          {t("localSessions.connectShort")}
        </Button>
      )}
      {open &&
        createPortal(
          <LocalSessionsDialog
            isDark={isDark}
            onClose={() => setOpen(false)}
            onSelectGroup={onSelectGroup}
          />,
          document.body,
        )}
    </>
  );
}
