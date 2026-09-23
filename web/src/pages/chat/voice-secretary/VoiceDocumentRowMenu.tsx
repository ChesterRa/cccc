import type { ReactNode } from "react";
import type { TFunction } from "i18next";
import { GroupItemMenuTrigger } from "../../../components/layout/GroupItemMenuTrigger";
import { useGroupMenu } from "../../../components/layout/useGroupMenu";

export function VoiceDocumentRowMenu({
  children,
  title,
  disabled,
  captureTarget = false,
  viewing = false,
  onSelect,
  onSetCaptureTarget,
  onArchive,
  onDelete,
  onMove,
  onRename,
  t,
}: {
  /** A function receives the row's own menu trigger to place inside the row. */
  children: ReactNode | ((trigger: ReactNode) => ReactNode);
  title: string;
  disabled: boolean;
  /** Whether this document already receives new transcript. */
  captureTarget?: boolean;
  viewing?: boolean;
  onSelect: () => void;
  onSetCaptureTarget?: () => void;
  onArchive?: () => void;
  onDelete?: () => void;
  onMove?: () => void;
  onRename?: () => void;
  t: TFunction;
}) {
  const menu = useGroupMenu(title, [
    {
      label: t("voiceSecretaryDocumentSelect", { defaultValue: "Select" }),
      onClick: onSelect,
      disabled,
    },
    ...(onSetCaptureTarget
      ? [
          {
            label: t("voiceSecretarySetCaptureTarget", { defaultValue: "Use by default" }),
            onClick: onSetCaptureTarget,
            disabled: disabled || captureTarget,
          },
        ]
      : []),
    ...(onRename
      ? [
          {
            label: t("voiceSecretaryDocumentRenameAction", { defaultValue: "Rename" }),
            onClick: onRename,
            disabled,
          },
        ]
      : []),
    {
      label: t("voiceSecretaryDocumentArchiveAction", { defaultValue: "Archive" }),
      onClick: () => onArchive?.(),
      disabled: disabled || !onArchive,
    },
    ...(onMove
      ? [
          {
            label: t("voiceFolderMove", { defaultValue: "Move to folder" }),
            onClick: onMove,
            disabled,
          },
        ]
      : []),
    {
      label: t("voiceSecretaryDocumentDeleteAction", { defaultValue: "Delete" }),
      onClick: () => onDelete?.(),
      disabled: disabled || !onDelete,
      tone: "danger",
      section: "delete",
    },
  ]);
  const trigger = (
    <GroupItemMenuTrigger
      isActive={viewing}
      label={t("voiceSecretaryDocumentActions", { title, defaultValue: "Actions for {{title}}" })}
      open={menu.open}
      onToggle={menu.toggle}
    />
  );
  return (
    <div
      onContextMenu={menu.onContextMenu}
      onKeyDownCapture={(event) => {
        if (menu.onKeyDown(event)) event.stopPropagation();
      }}
      tabIndex={-1}
    >
      {typeof children === "function" ? children(trigger) : children}
      {menu.menu}
    </div>
  );
}
