import { requestWorkspaceNavigation } from "../../../stores/workspaceNavigation";
import { useEffect, useId, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import * as api from "../../../services/api";
import { useGroupStore } from "../../../stores/useGroupStore";
import { CheckIcon, DownloadIcon, FileIcon } from "../../Icons";
import { CreateGroupDirectoryBrowser } from "../CreateGroupDirectoryBrowser";
import { driveSuggestions } from "../createGroupDirectoryModel";
import { useWorkspaceDirectoryPicker } from "./useWorkspaceDirectoryPicker";
import { inputClass, primaryButtonClass, secondaryButtonClass } from "./types";
import { SettingsCard, SettingsPage, SettingsRow } from "./SettingsKit";

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

export function CopyGroupsTab({ groupId, groupTitle }: CopyGroupsTabProps) {
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
  const {
    open: workspacePickerOpen,
    items: workspaceDirItems,
    currentDir: workspaceCurrentDir,
    parentDir: workspaceParentDir,
    locations: workspaceLocations,
    error: workspacePickerError,
    busy: workspacePickerBusy,
    show: openWorkspacePicker,
    close: closeWorkspacePicker,
    fetchDirectory: fetchWorkspaceDirectory,
    createDirectory: createWorkspaceDirectory,
  } = useWorkspaceDirectoryPicker();
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
      previewRequestSeqRef.current += 1;
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
        if (requestId === previewRequestSeqRef.current) {
          setErr(resp.error?.message || t("copyGroups.failedToPreview"));
        }
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
    const requestId = ++previewRequestSeqRef.current;
    setBusy(true);
    setErr("");
    setInfo("");
    try {
      const resp = await api.exportGroupCopy(groupId);
      if (!resp.ok) {
        if (requestId === previewRequestSeqRef.current) {
          setErr(resp.error?.message || t("copyGroups.failedToExport"));
        }
        return;
      }
      downloadBlobFile(
        resp.result.filename || `cccc-group--${groupTitle || groupId}.zip`,
        resp.result.blob,
      );
      if (requestId !== previewRequestSeqRef.current) return;
      setInfo(t("copyGroups.downloaded"));
      window.setTimeout(() => {
        if (requestId === previewRequestSeqRef.current) setInfo("");
      }, 1400);
    } catch {
      if (requestId === previewRequestSeqRef.current) setErr(t("copyGroups.failedToExport"));
    } finally {
      if (requestId === previewRequestSeqRef.current) setBusy(false);
    }
  };

  const handleImport = async () => {
    if (!copyFile || !copyPreview?.preview) return;
    const ok = window.confirm(t("copyGroups.importConfirm", { filename: copyFile.name }));
    if (!ok) return;

    const requestId = ++previewRequestSeqRef.current;
    const selectedGroupId = useGroupStore.getState().selectedGroupId;
    const isCurrent = () => requestId === previewRequestSeqRef.current;
    const canNavigate = () =>
      isCurrent() && useGroupStore.getState().selectedGroupId === selectedGroupId;
    setBusy(true);
    setErr("");
    setInfo("");
    try {
      const uploadId =
        copyPreview.file === copyFile ? String(copyPreview.upload_id || "").trim() : "";
      const resp = await api.importGroupCopy(
        { file: copyFile, uploadId },
        workspaceRoot.trim(),
        targetTitle.trim(),
      );
      if (!resp.ok) {
        if (isCurrent()) setErr(resp.error?.message || t("copyGroups.failedToImport"));
        return;
      }
      const nextGroupId = String(resp.result?.group_id || "").trim();
      stagedUploadIdRef.current = "";
      if (isCurrent()) {
        setCopyFile(null);
        setCopyPreview(null);
        setWorkspaceRoot("");
        setTargetTitle("");
        setInfo(t("copyGroups.imported"));
      }
      // Import creates a Group independently of the current workspace selection.
      await refreshGroups();
      if (!isCurrent()) return;
      if (nextGroupId && canNavigate()) {
        requestWorkspaceNavigation(() => {
          if (!canNavigate()) return;
          setSelectedGroupId(nextGroupId);
          void loadGroup(nextGroupId);
        });
      }
      window.setTimeout(() => {
        if (requestId === previewRequestSeqRef.current) setInfo("");
      }, 1600);
    } catch {
      if (requestId === previewRequestSeqRef.current) setErr(t("copyGroups.failedToImport"));
    } finally {
      if (requestId === previewRequestSeqRef.current) setBusy(false);
    }
  };

  const filePickerId = "group-copy-import-file-input";
  const workspaceRootId = useId();
  const targetTitleId = useId();
  const previewData = copyPreview?.preview;
  const actors = Array.isArray(previewData?.actors) ? previewData?.actors || [] : [];
  const sourceRoot = String(previewData?.source_workspace_root || "").trim();
  const canReset = !!copyFile || !!copyPreview;
  const previewSummaryItems = previewData
    ? [
        t("copyGroups.actors", { count: Number(previewData.actor_count || actors.length || 0) }),
        previewData.group_id_conflict
          ? t("copyGroups.conflictCopy")
          : t("copyGroups.conflictPreserve"),
        t("copyGroups.runtimeReset"),
        t("copyGroups.workspaceExcluded"),
        t("copyGroups.secretsExcluded"),
        previewData.requires_reconnect?.native_sessions ||
        actors.some((actor) => actor.native_session)
          ? t("copyGroups.reconnectNativeSessions")
          : "",
        previewData.requires_reconnect?.chatgpt_web_model ? t("copyGroups.reconnectChatGPT") : "",
        previewData.requires_reconnect?.grok_web_model ? t("copyGroups.reconnectGrok") : "",
        previewData.requires_reconnect?.notebooklm_group_space
          ? t("copyGroups.reconnectNotebookLM")
          : "",
      ].filter((item): item is string => Boolean(item))
    : [];
  const resetImport = () => {
    cleanupStagedCopy(copyPreview);
    previewRequestSeqRef.current += 1;
    setCopyFile(null);
    setCopyPreview(null);
    setWorkspaceRoot("");
    setTargetTitle("");
    setErr("");
    setInfo("");
  };

  return (
    <SettingsPage title={t("copyGroups.title")} description={t("copyGroups.description")}>
      {info || err || busy ? (
        <div className="flex flex-wrap items-center gap-2 text-sm" data-copy-groups-status>
          {info ? (
            <span
              role="status"
              className="inline-flex items-center gap-1.5 rounded-full border border-emerald-500/20 bg-emerald-500/10 px-3 py-1 text-xs font-medium text-emerald-700 dark:text-emerald-300"
            >
              <CheckIcon size={13} />
              {info}
            </span>
          ) : null}
          {busy ? (
            <span className="text-xs text-[var(--color-text-muted)]">
              {t("copyGroups.working")}
            </span>
          ) : null}
          {err ? (
            <div
              role="alert"
              className="w-full rounded-lg border border-rose-500/20 bg-rose-500/10 px-3 py-2 text-rose-700 dark:text-rose-300"
            >
              {err}
            </div>
          ) : null}
        </div>
      ) : null}

      <SettingsCard
        title={t("copyGroups.exportHeading")}
        description={t("copyGroups.exportDescription")}
        actions={
          <button
            type="button"
            className={primaryButtonClass(busy || !groupId)}
            onClick={handleExport}
            disabled={busy || !groupId}
            title={!groupId ? t("copyGroups.openFromGroup") : ""}
          >
            <DownloadIcon size={16} />
            {t("copyGroups.exportButton")}
          </button>
        }
      />

      <SettingsCard
        title={t("copyGroups.importHeading")}
        description={t("copyGroups.importDescription")}
        actions={
          <>
            {canReset ? (
              <button
                type="button"
                className={secondaryButtonClass("sm")}
                disabled={busy}
                onClick={resetImport}
              >
                {t("common:reset")}
              </button>
            ) : null}
            <button
              type="button"
              className={primaryButtonClass(busy || !copyFile || !copyPreview)}
              disabled={busy || !copyFile || !copyPreview}
              onClick={handleImport}
              title={!copyPreview ? t("copyGroups.pickFileFirst") : ""}
            >
              {t("copyGroups.importButton")}
            </button>
          </>
        }
      >
        <div className="flex min-w-0 flex-wrap items-center gap-2 px-5 py-3.5">
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
          <label
            htmlFor={filePickerId}
            className={`${secondaryButtonClass("sm")} ${busy ? "pointer-events-none opacity-50" : "cursor-pointer"}`}
            aria-disabled={busy}
          >
            <FileIcon size={16} />
            {t("common:chooseFile", "Choose File")}
          </label>
          <span
            className={`min-w-0 flex-1 truncate text-[13px] ${copyFile ? "text-[var(--color-text-primary)]" : "text-[var(--color-text-muted)]"}`}
          >
            {copyFile?.name || t("common:noFileChosen", "No file chosen")}
          </span>
        </div>

        {previewData ? (
          <>
            <SettingsRow
              label={t("copyGroups.workspaceRoot")}
              help={t("copyGroups.workspaceNote")}
              htmlFor={workspaceRootId}
            >
              <div className="flex w-full max-w-[520px] min-w-0 gap-2">
                <input
                  id={workspaceRootId}
                  className={`${inputClass()} min-w-0 flex-1`}
                  value={workspaceRoot}
                  onChange={(e) => setWorkspaceRoot(e.target.value)}
                  placeholder={sourceRoot || t("copyGroups.workspacePlaceholder")}
                />
                <button
                  type="button"
                  className={secondaryButtonClass("sm")}
                  disabled={busy || workspacePickerBusy}
                  onClick={() => void openWorkspacePicker(workspaceRoot.trim() || sourceRoot)}
                >
                  {t("copyGroups.chooseFolder")}
                </button>
              </div>
              {workspacePickerOpen ? (
                <div className="w-full space-y-2 rounded-xl border border-[var(--glass-border-subtle)] bg-[var(--glass-panel-bg)] p-2">
                  <CreateGroupDirectoryBrowser
                    dirItems={workspaceDirItems}
                    currentDir={workspaceCurrentDir}
                    parentDir={workspaceParentDir}
                    driveLocations={driveSuggestions(workspaceLocations)}
                    error={workspacePickerError}
                    creatingDirectory={workspacePickerBusy}
                    onSelect={() => {}}
                    onFetch={(path) => void fetchWorkspaceDirectory(path)}
                    onCreateDirectory={createWorkspaceDirectory}
                  />
                  <div className="flex items-center justify-between gap-2">
                    <span className="min-w-0 truncate font-mono text-xs text-[var(--color-text-muted)]">
                      {workspaceCurrentDir}
                    </span>
                    <div className="flex shrink-0 gap-2">
                      <button
                        type="button"
                        className={secondaryButtonClass("sm")}
                        onClick={() => closeWorkspacePicker()}
                      >
                        {t("common:cancel")}
                      </button>
                      <button
                        type="button"
                        className={primaryButtonClass(!workspaceCurrentDir || workspacePickerBusy)}
                        disabled={!workspaceCurrentDir || workspacePickerBusy}
                        onClick={() => {
                          setWorkspaceRoot(workspaceCurrentDir);
                          closeWorkspacePicker();
                        }}
                      >
                        {t("copyGroups.useFolder")}
                      </button>
                    </div>
                  </div>
                </div>
              ) : null}
            </SettingsRow>

            <SettingsRow label={t("copyGroups.groupTitle")} htmlFor={targetTitleId}>
              <input
                id={targetTitleId}
                className={`${inputClass()} max-w-[360px]`}
                value={targetTitle}
                onChange={(e) => setTargetTitle(e.target.value)}
                placeholder={String(previewData.source_title || "")}
              />
            </SettingsRow>

            <div className="grid min-w-0 gap-3 px-5 py-3.5" data-copy-groups-preview>
              <div className="min-w-0">
                <div className="text-[13px] font-semibold text-[var(--color-text-primary)]">
                  {String(previewData.source_title || t("copyGroups.untitled"))}
                </div>
                <div className="mt-0.5 text-xs text-[var(--color-text-muted)]">
                  {t("copyGroups.sourceId", { groupId: previewData.source_group_id || "unknown" })}
                </div>
              </div>
              <ul className="grid gap-1.5 @min-[900px]/settings:grid-cols-2">
                {previewSummaryItems.map((item) => (
                  <li
                    key={item}
                    className="flex items-start gap-2 text-xs leading-5 text-[var(--color-text-secondary)]"
                  >
                    <CheckIcon
                      size={13}
                      className="mt-1 shrink-0 text-emerald-600 dark:text-emerald-300"
                    />
                    <span>{item}</span>
                  </li>
                ))}
              </ul>
            </div>
          </>
        ) : null}
      </SettingsCard>
    </SettingsPage>
  );
}
