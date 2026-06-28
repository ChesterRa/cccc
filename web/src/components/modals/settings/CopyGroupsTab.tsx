import { useEffect, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import * as api from "../../../services/api";
import { useGroupStore } from "../../../stores/useGroupStore";
import { CheckIcon, DownloadIcon, FileIcon } from "../../Icons";
import {
  labelClass,
  primaryButtonClass,
  secondaryButtonClass,
  settingsWorkspaceHeaderClass,
  settingsWorkspaceShellClass,
} from "./types";

interface CopyGroupsTabProps {
  isDark: boolean;
  groupId?: string;
  groupTitle?: string;
}

type CopyPreviewResult = {
  preview?: api.GroupCopyPreview;
  upload_id?: string;
  package_size_bytes?: number;
  file?: File;
};

function downloadBlobFile(filename: string, blob: Blob) {
  const url = URL.createObjectURL(blob);
  const a = document.createElement("a");
  a.href = url;
  a.download = filename;
  document.body.appendChild(a);
  a.click();
  document.body.removeChild(a);
  URL.revokeObjectURL(url);
}

export function CopyGroupsTab({ isDark, groupId, groupTitle }: CopyGroupsTabProps) {
  const { t } = useTranslation("settings");
  const refreshGroups = useGroupStore((state) => state.refreshGroups);
  const setSelectedGroupId = useGroupStore((state) => state.setSelectedGroupId);
  const loadGroup = useGroupStore((state) => state.loadGroup);

  const [copyFile, setCopyFile] = useState<File | null>(null);
  const [copyPreview, setCopyPreview] = useState<CopyPreviewResult | null>(null);
  const [workspaceRoot, setWorkspaceRoot] = useState("");
  const [targetTitle, setTargetTitle] = useState("");
  const [err, setErr] = useState("");
  const [busy, setBusy] = useState(false);
  const [info, setInfo] = useState("");
  const stagedUploadIdRef = useRef("");
  const previewRequestSeqRef = useRef(0);

  const cleanupStagedUploadId = (uploadId: string) => {
    const trimmed = String(uploadId || "").trim();
    if (!trimmed) return;
    if (stagedUploadIdRef.current === trimmed) stagedUploadIdRef.current = "";
    void api.cleanupGroupCopyUpload(trimmed);
  };

  const cleanupStagedCopy = (preview: CopyPreviewResult | null) => {
    cleanupStagedUploadId(String(preview?.upload_id || ""));
  };

  useEffect(() => {
    return () => {
      const uploadId = stagedUploadIdRef.current;
      stagedUploadIdRef.current = "";
      if (uploadId) void api.cleanupGroupCopyUpload(uploadId);
    };
  }, []);

  const loadCopyPreview = async (file: File) => {
    const requestId = previewRequestSeqRef.current + 1;
    previewRequestSeqRef.current = requestId;
    setBusy(true);
    setErr("");
    setInfo("");
    setCopyPreview(null);
    setWorkspaceRoot("");
    setTargetTitle("");
    try {
      const resp = await api.previewGroupCopy(file);
      if (!resp.ok) {
        setErr(resp.error?.message || t("copyGroups.failedToPreview"));
        return;
      }
      const nextPreview = resp.result as CopyPreviewResult;
      const nextUploadId = String(nextPreview.upload_id || "").trim();
      if (requestId !== previewRequestSeqRef.current) {
        cleanupStagedUploadId(nextUploadId);
        return;
      }
      const previewData = nextPreview.preview;
      stagedUploadIdRef.current = nextUploadId;
      setCopyPreview({ ...nextPreview, file });
      setWorkspaceRoot(String(previewData?.source_workspace_root || ""));
      setTargetTitle(String(previewData?.source_title || ""));
    } catch {
      if (requestId === previewRequestSeqRef.current) setErr(t("copyGroups.failedToPreview"));
    } finally {
      if (requestId === previewRequestSeqRef.current) setBusy(false);
    }
  };

  const handleExport = async () => {
    if (!groupId) return;
    setBusy(true);
    setErr("");
    setInfo("");
    try {
      const resp = await api.exportGroupCopy(groupId);
      if (!resp.ok) {
        setErr(resp.error?.message || t("copyGroups.failedToExport"));
        return;
      }
      downloadBlobFile(resp.result.filename || `cccc-group--${groupTitle || groupId}.zip`, resp.result.blob);
      setInfo(t("copyGroups.downloaded"));
      window.setTimeout(() => setInfo(""), 1400);
    } catch {
      setErr(t("copyGroups.failedToExport"));
    } finally {
      setBusy(false);
    }
  };

  const handleImport = async () => {
    if (!copyFile || !copyPreview?.preview) return;
    const ok = window.confirm(t("copyGroups.importConfirm", { filename: copyFile.name }));
    if (!ok) return;

    setBusy(true);
    setErr("");
    setInfo("");
    try {
      const uploadId = copyPreview.file === copyFile ? String(copyPreview.upload_id || "").trim() : "";
      const resp = await api.importGroupCopy(
        { file: copyFile, uploadId },
        workspaceRoot.trim(),
        targetTitle.trim(),
      );
      if (!resp.ok) {
        setErr(resp.error?.message || t("copyGroups.failedToImport"));
        return;
      }
      const nextGroupId = String(resp.result?.group_id || "").trim();
      stagedUploadIdRef.current = "";
      setCopyFile(null);
      setCopyPreview(null);
      setWorkspaceRoot("");
      setTargetTitle("");
      setInfo(t("copyGroups.imported"));
      await refreshGroups();
      if (nextGroupId) {
        setSelectedGroupId(nextGroupId);
        await loadGroup(nextGroupId);
      }
      window.setTimeout(() => setInfo(""), 1600);
    } catch {
      setErr(t("copyGroups.failedToImport"));
    } finally {
      setBusy(false);
    }
  };

  const filePickerId = "group-copy-import-file-input";
  const previewData = copyPreview?.preview;
  const actors = Array.isArray(previewData?.actors) ? previewData?.actors || [] : [];
  const sourceRoot = String(previewData?.source_workspace_root || "").trim();
  const canReset = !!copyFile || !!copyPreview;
  const sectionTitleClass = "text-xs font-semibold uppercase tracking-[0.16em] text-[var(--color-text-tertiary)]";
  const previewSummaryItems = previewData
    ? [
        t("copyGroups.actors", { count: Number(previewData.actor_count || actors.length || 0) }),
        previewData.group_id_conflict ? t("copyGroups.conflictCopy") : t("copyGroups.conflictPreserve"),
        t("copyGroups.runtimeReset"),
        t("copyGroups.workspaceExcluded"),
        t("copyGroups.secretsExcluded"),
        previewData.requires_reconnect?.chatgpt_web_model ? t("copyGroups.reconnectChatGPT") : "",
        previewData.requires_reconnect?.notebooklm_group_space ? t("copyGroups.reconnectNotebookLM") : "",
      ].filter((item): item is string => Boolean(item))
    : [];

  return (
    <div className={settingsWorkspaceShellClass(isDark)}>
      <div className={settingsWorkspaceHeaderClass(isDark)}>
        <div className="min-w-0 flex-1">
          <div className="text-sm font-semibold text-[var(--color-text-primary)]">{t("copyGroups.title")}</div>
          <div className="mt-1 max-w-3xl text-xs leading-5 text-[var(--color-text-tertiary)]">{t("copyGroups.description")}</div>
        </div>
        {info ? (
          <div className="inline-flex shrink-0 items-center gap-1.5 rounded-full border border-emerald-500/20 bg-emerald-500/10 px-3 py-1 text-xs font-medium text-emerald-700 dark:text-emerald-300">
            <CheckIcon size={13} />
            {info}
          </div>
        ) : null}
      </div>

      <div className="divide-y divide-[var(--glass-border-subtle)]">
        {err ? (
          <div className="px-4 py-3 sm:px-5">
            <div className="rounded-lg border border-rose-500/20 bg-rose-500/10 px-3 py-2 text-sm text-rose-700 dark:text-rose-300">{err}</div>
          </div>
        ) : null}

        <section className="grid gap-4 px-4 py-4 sm:px-5 sm:py-5 lg:grid-cols-[minmax(0,1fr)_auto] lg:items-center">
          <div className="min-w-0">
            <div className={sectionTitleClass}>{t("copyGroups.exportHeading")}</div>
            <p className="mt-2 max-w-3xl text-sm leading-6 text-[var(--color-text-secondary)]">{t("copyGroups.exportDescription")}</p>
          </div>
          <button
            type="button"
            className={`${primaryButtonClass(busy || !groupId)} w-fit`}
            onClick={handleExport}
            disabled={busy || !groupId}
            title={!groupId ? t("copyGroups.openFromGroup") : ""}
          >
            <DownloadIcon size={16} />
            {t("copyGroups.exportButton")}
          </button>
        </section>

        <section className="px-4 py-4 sm:px-5 sm:py-5">
          <div className="grid gap-4 xl:grid-cols-[minmax(0,0.88fr)_minmax(360px,1fr)] xl:items-start">
            <div className="min-w-0">
              <div className={sectionTitleClass}>{t("copyGroups.importHeading")}</div>
              <p className="mt-2 max-w-3xl text-sm leading-6 text-[var(--color-text-secondary)]">{t("copyGroups.importDescription")}</p>
            </div>

            <div className="min-w-0 space-y-3">
              <input
                key={copyFile ? copyFile.name : "none"}
                id={filePickerId}
                type="file"
                accept=".zip,application/zip"
                className="sr-only"
                disabled={busy}
                onChange={(e) => {
                  const file = e.target.files && e.target.files.length > 0 ? e.target.files[0] : null;
                  cleanupStagedCopy(copyPreview);
                  previewRequestSeqRef.current += 1;
                  setCopyFile(file);
                  setCopyPreview(null);
                  setWorkspaceRoot("");
                  setTargetTitle("");
                  setErr("");
                  setInfo("");
                  if (file) void loadCopyPreview(file);
                }}
              />

              <div className="grid gap-2 sm:grid-cols-[auto_minmax(0,1fr)_auto] sm:items-center">
                <label
                  htmlFor={filePickerId}
                  className={`${secondaryButtonClass()} ${busy ? "pointer-events-none opacity-50" : "cursor-pointer"}`}
                  aria-disabled={busy}
                >
                  <FileIcon size={16} />
                  {t("common:chooseFile", "Choose File")}
                </label>
                <div className="min-w-0 rounded-xl border border-[var(--glass-border-subtle)] bg-[var(--glass-panel-bg)] px-3 py-2.5 text-sm">
                  <span className={`block truncate ${copyFile ? "text-[var(--color-text-primary)]" : "text-[var(--color-text-muted)]"}`}>
                    {copyFile?.name || t("common:noFileChosen", "No file chosen")}
                  </span>
                </div>
                <button
                  type="button"
                  className={primaryButtonClass(busy || !copyFile || !copyPreview)}
                  disabled={busy || !copyFile || !copyPreview}
                  onClick={handleImport}
                  title={!copyPreview ? t("copyGroups.pickFileFirst") : ""}
                >
                  {t("copyGroups.importButton")}
                </button>
              </div>

              {canReset || busy ? (
                <div className="flex flex-wrap items-center justify-between gap-2 text-xs">
                  <div className="text-[var(--color-text-muted)]">{busy ? t("copyGroups.working") : "\u00a0"}</div>
                  {canReset ? (
                    <button
                      type="button"
                      className={secondaryButtonClass("sm")}
                      disabled={busy}
                      onClick={() => {
                        cleanupStagedCopy(copyPreview);
                        previewRequestSeqRef.current += 1;
                        setCopyFile(null);
                        setCopyPreview(null);
                        setWorkspaceRoot("");
                        setTargetTitle("");
                        setErr("");
                        setInfo("");
                      }}
                    >
                      {t("common:reset")}
                    </button>
                  ) : null}
                </div>
              ) : null}
            </div>
          </div>

          {previewData ? (
            <div className="mt-4 rounded-xl border border-[var(--glass-border-subtle)] bg-[var(--glass-panel-bg)] p-3.5 sm:p-4">
              <div className="grid gap-4 lg:grid-cols-[minmax(0,1fr)_minmax(260px,0.75fr)]">
                <div className="min-w-0 space-y-3">
                  <div>
                    <div className="text-sm font-semibold text-[var(--color-text-primary)]">
                      {String(previewData.source_title || t("copyGroups.untitled"))}
                    </div>
                    <div className="mt-1 text-xs text-[var(--color-text-tertiary)]">
                      {t("copyGroups.sourceId", { groupId: previewData.source_group_id || "unknown" })}
                    </div>
                  </div>
                  <div className="grid gap-2 sm:grid-cols-2">
                    <div>
                      <label className={labelClass(isDark)}>{t("copyGroups.workspaceRoot")}</label>
                      <input
                        className="mt-1 w-full rounded-lg border border-[var(--color-border)] bg-[var(--color-surface)] px-3 py-2 text-sm text-[var(--color-text-primary)] outline-none focus:border-[var(--color-accent)]"
                        value={workspaceRoot}
                        onChange={(e) => setWorkspaceRoot(e.target.value)}
                        placeholder={sourceRoot || t("copyGroups.workspacePlaceholder")}
                      />
                    </div>
                    <div>
                      <label className={labelClass(isDark)}>{t("copyGroups.groupTitle")}</label>
                      <input
                        className="mt-1 w-full rounded-lg border border-[var(--color-border)] bg-[var(--color-surface)] px-3 py-2 text-sm text-[var(--color-text-primary)] outline-none focus:border-[var(--color-accent)]"
                        value={targetTitle}
                        onChange={(e) => setTargetTitle(e.target.value)}
                        placeholder={String(previewData.source_title || "")}
                      />
                    </div>
                  </div>
                  <div className="text-xs leading-5 text-[var(--color-text-tertiary)]">{t("copyGroups.workspaceNote")}</div>
                </div>

                <div className="grid content-start gap-2">
                  {previewSummaryItems.map((item) => (
                    <div
                      key={item}
                      className="flex items-start gap-2 rounded-lg border border-[var(--glass-border-subtle)] bg-[var(--color-surface)] px-3 py-2 text-xs text-[var(--color-text-secondary)]"
                    >
                      <CheckIcon size={13} className="mt-0.5 shrink-0 text-emerald-600 dark:text-emerald-300" />
                      <span>{item}</span>
                    </div>
                  ))}
                </div>
              </div>
            </div>
          ) : null}
        </section>
      </div>
    </div>
  );
}
