import { useTranslation } from "react-i18next";
import { useCopyFeedback } from "../../hooks/useCopyFeedback";
import { useModalA11y } from "../../hooks/useModalA11y";
import { useIMEComposition } from "../../hooks/useIMEComposition";
import { Button } from "../ui/button";
import { Input } from "../ui/input";
import { Surface } from "../ui/surface";
import { Textarea } from "../ui/textarea";
import { ModalFrame } from "./ModalFrame";

export interface GroupEditModalProps {
  isOpen: boolean;
  isDark: boolean;
  busy: string;
  groupId: string;
  ccccHome: string;
  projectRoot: string;
  title: string;
  topic: string;
  onChangeTitle: (title: string) => void;
  onChangeTopic: (topic: string) => void;
  onSave: () => void;
  onCancel: () => void;
  onReset: () => void;
  onDelete: () => void;
}

export function GroupEditModal({
  isOpen,
  isDark,
  busy,
  groupId,
  ccccHome,
  projectRoot,
  title,
  topic,
  onChangeTitle,
  onChangeTopic,
  onSave,
  onCancel,
  onReset,
  onDelete,
}: GroupEditModalProps) {
  const { t } = useTranslation("modals");
  const copyWithFeedback = useCopyFeedback();
  const { modalRef } = useModalA11y(isOpen, onCancel);
  const imeTitle = useIMEComposition({ value: title, onChange: onChangeTitle });
  const imeTopic = useIMEComposition({ value: topic, onChange: onChangeTopic });
  if (!isOpen) return null;

  const homeRoot = String(ccccHome || "").trim();
  const gid = String(groupId || "").trim();
  const groupDataDir = homeRoot && gid ? `${homeRoot}/groups/${gid}` : "";
  const groupConfigFile = groupDataDir ? `${groupDataDir}/group.yaml` : "";
  const groupLedgerFile = groupDataDir ? `${groupDataDir}/ledger.jsonl` : "";
  const metadataRows = [
    {
      label: t("groupEdit.groupId"),
      value: groupId || "—",
      copyValue: groupId,
      title: t("groupEdit.copyGroupId"),
    },
    {
      label: t("groupEdit.projectRoot"),
      value: projectRoot || t("groupEdit.noScopeAttached"),
      copyValue: projectRoot,
      title: t("groupEdit.copyProjectRoot"),
    },
    {
      label: t("groupEdit.groupDataDirectory"),
      value: groupDataDir || "—",
      copyValue: groupDataDir,
      title: t("groupEdit.copyDataDir"),
    },
    {
      label: t("groupEdit.groupConfigFile"),
      value: groupConfigFile || "—",
      copyValue: groupConfigFile,
      title: t("groupEdit.copyConfigFile"),
    },
    {
      label: t("groupEdit.groupLedgerFile"),
      value: groupLedgerFile || "—",
      copyValue: groupLedgerFile,
      title: t("groupEdit.copyLedgerFile"),
    },
  ];

  return (
    <ModalFrame
      isOpen={isOpen}
      isDark={isDark}
      onClose={onCancel}
      titleId="group-edit-title"
      title={
        <div className="text-xl font-semibold text-[var(--color-text-primary)]">
          {t("groupEdit.title")}
        </div>
      }
      closeAriaLabel={t("common:close")}
      panelClassName="w-full h-full sm:h-auto sm:max-w-2xl sm:mt-12 sm:max-h-[calc(100dvh-6rem)]"
      modalRef={modalRef}
      footerActions={
        <div className="flex flex-col-reverse sm:flex-row gap-3 sm:items-center sm:justify-between w-full">
          <div className="flex flex-col gap-2 w-full sm:w-auto">
            <div className="flex flex-col gap-3 sm:flex-row">
              <Button
                type="button"
                variant="outline"
                className="w-full sm:w-auto transition-all ease-spring duration-300"
                onClick={onReset}
                disabled={busy === "group-reset"}
                title={t("groupEdit.resetTitle")}
              >
                {t("groupEdit.resetGroup")}
              </Button>
              <Button
                type="button"
                variant="destructive"
                className="w-full sm:w-auto transition-all ease-spring duration-300"
                onClick={() => {
                  onCancel();
                  onDelete();
                }}
                disabled={busy === "group-delete"}
                title={t("groupEdit.deleteTitle")}
              >
                {t("groupEdit.deleteGroup")}
              </Button>
            </div>
            <div className="max-w-sm text-xs leading-5 text-[var(--color-text-muted)]">
              {t("groupEdit.resetHint")}
            </div>
          </div>
          <div className="flex flex-col-reverse sm:flex-row gap-3 w-full sm:w-auto sm:justify-end">
            <Button
              type="button"
              variant="secondary"
              className="w-full sm:w-auto transition-all ease-spring duration-300"
              onClick={onCancel}
            >
              {t("common:cancel")}
            </Button>
            <Button
              type="button"
              className="w-full sm:w-auto font-semibold transition-all ease-spring duration-300"
              onClick={onSave}
              disabled={!title.trim() || busy === "group-update"}
            >
              {t("common:save")}
            </Button>
          </div>
        </div>
      }
    >
      <div className="scrollbar-hide flex-1 overflow-y-auto bg-[radial-gradient(circle_at_top,rgba(255,255,255,0.92),rgba(255,255,255,0)_28%),linear-gradient(180deg,rgb(251,250,247),rgb(245,244,241))] px-6 pb-6 pt-4 dark:bg-[radial-gradient(circle_at_top,rgba(255,255,255,0.05),rgba(255,255,255,0)_32%),linear-gradient(180deg,rgba(17,18,22,0.98),rgba(11,12,15,1))] sm:px-7 sm:pb-7 sm:pt-5">
        <div className="space-y-5">
          <div>
            <label className="mb-2 block text-xs font-medium uppercase tracking-[0.08em] text-[var(--color-text-muted)]">
              {t("groupEdit.nameLabel")}
            </label>
            <Input
              className="min-h-[44px]"
              value={imeTitle.value}
              onChange={imeTitle.onChange}
              onCompositionStart={imeTitle.onCompositionStart}
              onCompositionEnd={imeTitle.onCompositionEnd}
              placeholder={t("groupEdit.groupNamePlaceholder")}
            />
          </div>
          <div>
            <label className="mb-2 block text-xs font-medium uppercase tracking-[0.08em] text-[var(--color-text-muted)]">
              {t("groupEdit.descriptionLabel")}
            </label>
            <Textarea
              className="min-h-[92px] resize-none text-sm leading-6"
              value={imeTopic.value}
              onChange={imeTopic.onChange}
              onCompositionStart={imeTopic.onCompositionStart}
              onCompositionEnd={imeTopic.onCompositionEnd}
              placeholder={t("groupEdit.descriptionPlaceholder")}
            />
          </div>
          <Surface className="overflow-hidden border-black/8 bg-[linear-gradient(180deg,rgba(255,255,255,0.995),rgba(250,248,245,0.96))] shadow-[0_24px_60px_-40px_rgba(15,23,42,0.18)] dark:border-white/10 dark:bg-[linear-gradient(180deg,rgba(24,26,31,0.9),rgba(13,14,18,0.98))]" padding="none">
            <div className="border-b border-[var(--glass-border-subtle)] px-5 py-4 sm:px-6 bg-[rgba(18,18,20,0.018)] dark:bg-white/[0.03]">
              <div className="text-sm font-semibold text-[var(--color-text-primary)]">
                {t("groupEdit.projectRoot")}
              </div>
              <div className="mt-1 text-xs text-[var(--color-text-muted)]">
                {t("groupEdit.groupDataDirectory")} / {t("groupEdit.groupConfigFile")} / {t("groupEdit.groupLedgerFile")}
              </div>
            </div>
            <div className="divide-y divide-[var(--glass-border-subtle)]">
              {metadataRows.map((row) => (
                <div
                  key={row.label}
                  className="grid grid-cols-1 gap-3 px-5 py-3 sm:grid-cols-[140px_minmax(0,1fr)_auto] sm:items-center sm:gap-4 sm:px-6"
                >
                  <div className="text-[11px] font-medium uppercase tracking-[0.08em] text-[var(--color-text-muted)]">
                    {row.label}
                  </div>
                  <div className="min-w-0 font-mono text-[13px] leading-6 text-[var(--color-text-primary)] sm:truncate">
                    {row.value}
                  </div>
                  <Button
                    className="self-start sm:self-auto"
                    size="sm"
                    variant="outline"
                    onClick={async () => {
                      const ok = await copyWithFeedback(row.copyValue, {
                        successMessage: t("common:copied"),
                        errorMessage: t("common:copyFailed"),
                      });
                      if (!ok) return;
                    }}
                    disabled={!row.copyValue}
                    title={row.title}
                    type="button"
                  >
                    {t("common:copy")}
                  </Button>
                </div>
              ))}
            </div>
          </Surface>
        </div>
      </div>
    </ModalFrame>
  );
}
