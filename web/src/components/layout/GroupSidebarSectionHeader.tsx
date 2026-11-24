import type { ReactNode } from "react";
import { useTranslation } from "react-i18next";

export function GroupSidebarSectionHeader({ action }: { action?: ReactNode }) {
  const { t } = useTranslation("layout");
  // The list below is the only one in the sidebar and "New" names what it holds, so the
  // row shows no heading, only the action at the far end. The instance details live on
  // the product name in the header. Screen readers still hear the list named apart from
  // Archived.
  const label = <span className="sr-only">{t("workingGroups")}</span>;
  if (!action) return label;
  return (
    <div className="flex shrink-0 min-w-0 flex-wrap items-center gap-x-2 px-4 pt-2 pb-1">
      {label}
      <div className="ml-auto flex shrink-0 items-center">{action}</div>
    </div>
  );
}
