import type { TFunction } from "i18next";
import type { HTMLAttributes, Ref } from "react";
import { FileText } from "lucide-react";
import { VoiceDocumentRowMenu } from "./VoiceDocumentRowMenu";
import type { AssistantVoiceDocument } from "../../../types";
import { classNames } from "../../../utils/classNames";

/** Panel-level state and callbacks every document row reads. */
export type VoiceDocumentRowContext = {
  actionBusy: string;
  recording?: boolean;
  activeDocumentPath: string;
  captureTargetDocumentPath: string;
  isDark: boolean;
  t: TFunction;
  documentKey: (document: AssistantVoiceDocument) => string;
  documentPath: (document: AssistantVoiceDocument) => string;
  onSelectDocument: (document: AssistantVoiceDocument) => void;
  onSetCaptureTargetDocument: (document: AssistantVoiceDocument) => void;
  onArchiveDocument?: (document: AssistantVoiceDocument) => void;
  onDeleteDocument?: (document: AssistantVoiceDocument) => void;
  onMoveDocument?: (document: AssistantVoiceDocument) => void;
  onRenameDocument?: (document: AssistantVoiceDocument) => void;
};

export type VoiceDocumentRowDrag = {
  ref: Ref<HTMLDivElement>;
  props: HTMLAttributes<HTMLDivElement>;
  dragging: boolean;
};

export function VoiceDocumentRow({
  ctx,
  document,
  depth = 0,
  drag,
}: {
  ctx: VoiceDocumentRowContext;
  document: AssistantVoiceDocument;
  /** Tree nesting level; each level indents one folder-icon width. */
  depth?: number;
  drag?: VoiceDocumentRowDrag;
}) {
  const { isDark, t } = ctx;
  const docId = ctx.documentKey(document);
  const docPath = ctx.documentPath(document);
  const title = document.title || docId;
  const viewing = !!docPath && docPath === ctx.activeDocumentPath;
  const captureTarget = !!docPath && docPath === ctx.captureTargetDocumentPath;
  const select = () => ctx.onSelectDocument(document);
  return (
    <VoiceDocumentRowMenu
      title={title}
      disabled={!!ctx.actionBusy || !!ctx.recording}
      captureTarget={captureTarget}
      viewing={viewing}
      t={t}
      onSelect={select}
      onSetCaptureTarget={docPath ? () => ctx.onSetCaptureTargetDocument(document) : undefined}
      onArchive={ctx.onArchiveDocument ? () => ctx.onArchiveDocument!(document) : undefined}
      onDelete={ctx.onDeleteDocument ? () => ctx.onDeleteDocument!(document) : undefined}
      onMove={ctx.onMoveDocument ? () => ctx.onMoveDocument!(document) : undefined}
      onRename={ctx.onRenameDocument ? () => ctx.onRenameDocument!(document) : undefined}
    >
      {(trigger) => (
        <div
          ref={drag?.ref}
          {...drag?.props}
          role="button"
          tabIndex={0}
          onClick={select}
          onKeyDown={(event) => {
            if (event.target !== event.currentTarget) return;
            if (event.key !== "Enter" && event.key !== " ") return;
            event.preventDefault();
            select();
          }}
          style={{ paddingLeft: `${0.5 + depth * 1.25}rem` }}
          className={classNames(
            "group/item flex w-full min-w-0 items-center gap-1.5 rounded-lg py-1.5 pr-1 text-left text-sm transition-colors focus-visible:outline-none focus-visible:ring-2 pointer-coarse:py-2.5",
            drag?.dragging && "opacity-40",
            viewing
              ? isDark
                ? "bg-white/[0.1] font-semibold text-white focus-visible:ring-white/35"
                : "bg-white font-semibold text-[rgb(35,36,37)] shadow-[0_1px_3px_rgba(15,23,42,0.08)] focus-visible:ring-black/25"
              : isDark
                ? "text-slate-300 hover:bg-white/8 focus-visible:ring-white/35"
                : "text-gray-700 hover:bg-black/[0.04] focus-visible:ring-black/25",
          )}
        >
          <FileText size={15} aria-hidden="true" className="shrink-0 opacity-60" />
          <span title={title} className="min-w-0 flex-1 truncate">
            {title}
          </span>
          {captureTarget ? (
            <span
              data-voice-document-default
              className={classNames(
                "shrink-0 rounded-full px-1.5 py-0.5 text-[10px] font-semibold leading-none",
                isDark ? "bg-white/[0.14] text-white" : "bg-[rgb(35,36,37)] text-white",
              )}
            >
              {t("voiceSecretaryCaptureTargetBadge", { defaultValue: "Default" })}
            </span>
          ) : null}
          {trigger}
        </div>
      )}
    </VoiceDocumentRowMenu>
  );
}
