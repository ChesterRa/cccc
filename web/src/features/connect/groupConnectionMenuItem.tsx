import { useTranslation } from "react-i18next";
import { ConnectLinkIcon } from "../../components/Icons";
import type { GroupMenuActionItem } from "../../components/layout/useGroupMenu";
import { groupConnectionCountText, type GroupConnectionCount } from "./protocol";

/**
 * The Group connections entry of a sidebar row's menu. The row shows no connection
 * count of its own; the entry carries it, and opens the same list the count opened.
 */
export function useGroupConnectionMenuItem(
  label: string | undefined,
  connection: GroupConnectionCount | undefined,
  onOpen: (() => void) | undefined,
): GroupMenuActionItem[] {
  const { t } = useTranslation("layout");
  if (!onOpen || !label) return [];
  const text = groupConnectionCountText(connection);
  return [
    {
      label,
      detail: text?.value,
      ariaLabel: text
        ? t(text.current ? "groupConnections.count" : "groupConnections.countUnknown", {
            count: connection?.count,
          })
        : undefined,
      icon: <ConnectLinkIcon size={15} />,
      onClick: onOpen,
    },
  ];
}
