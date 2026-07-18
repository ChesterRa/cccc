import { useCallback, useEffect, useMemo, useState } from "react";
import { useTranslation } from "react-i18next";
import type { PresentationSlot, PresentationWorkspaceItem } from "../../types";
import { fetchPresentationWorkspaceListing } from "../../services/api";
import { useModalA11y } from "../../hooks/useModalA11y";
import { classNames } from "../../utils/classNames";
import { isValidPresentationWebUrl, normalizePresentationUrlInput } from "../../utils/presentation";
import { ModalFrame } from "../modals/ModalFrame";

type PresentationPinModalProps = {
  isOpen: boolean;
  isDark: boolean;
  groupId: string;
  slot: PresentationSlot | null;
  busy: boolean;
  onClose: () => void;
  onSubmitUrl: (payload: {
    slotId: string;
    url: string;
    title: string;
    summary: string;
  }) => Promise<void> | void;
  onSubmitWorkspace: (payload: {
    slotId: string;
    path: string;
    title: string;
    summary: string;
  }) => Promise<void> | void;
  onSubmitFile: (payload: {
    slotId: string;
    file: File;
    title: string;
    summary: string;
  }) => Promise<void> | void;
};

type PinSource = "url" | "workspace" | "upload";

function dirname(pathText: string): string {
  const parts = String(pathText || "")
    .trim()
    .replace(/\\/g, "/")
    .split("/")
    .filter(Boolean);
  if (parts.length <= 1) return "";
  return parts.slice(0, -1).join("/");
}

function initialSource(slot: PresentationSlot | null): PinSource {
  const card = slot?.card;
  if (card?.content?.url) return "url";
  if (card?.content?.workspace_rel_path || card?.content?.mode === "workspace_link")
    return "workspace";
  return card ? "upload" : "url";
}

function initialWorkspacePath(slot: PresentationSlot | null): string {
  return String(slot?.card?.content?.workspace_rel_path || "").trim();
}

function buildWorkspacePathLabel(rootPath: string, relativePath: string): string {
  const root = String(rootPath || "").trim();
  const rel = String(relativePath || "").trim();
  if (!root) return rel || "/";
  if (!rel) return root;
  return `${root}/${rel}`;
}

function WorkspaceList({
  isDark,
  rootPath,
  currentPath,
  parentPath,
  items,
  selectedPath,
  busy,
  error,
  onOpenDir,
  onSelectFile,
}: {
  isDark: boolean;
  rootPath: string;
  currentPath: string;
  parentPath: string | null;
  items: PresentationWorkspaceItem[];
  selectedPath: string;
  busy: boolean;
  error: string;
  onOpenDir: (path: string) => void;
  onSelectFile: (path: string) => void;
}) {
  return (
    <div
      className={classNames(
        "overflow-hidden rounded-2xl border",
        isDark ? "border-white/10 bg-slate-950/50" : "border-black/10 bg-white/90",
      )}
    >
      <div
        className={classNames(
          "border-b px-4 py-2 text-xs font-mono",
          isDark ? "border-white/10 text-slate-400" : "border-black/10 text-gray-500",
        )}
      >
        {buildWorkspacePathLabel(rootPath, currentPath)}
      </div>
      {error ? (
        <div
          className={classNames("px-4 py-4 text-sm", isDark ? "text-rose-300" : "text-rose-600")}
        >
          {error}
        </div>
      ) : (
        <div className="max-h-64 overflow-auto">
          {parentPath !== null ? (
            <button
              type="button"
              onClick={() => onOpenDir(parentPath)}
              className={classNames(
                "flex w-full items-center gap-2 border-b px-4 py-3 text-left text-sm transition-colors",
                isDark
                  ? "border-white/10 text-slate-300 hover:bg-slate-900/70"
                  : "border-black/10 text-gray-700 hover:bg-gray-50",
              )}
            >
              <span className={isDark ? "text-slate-500" : "text-gray-400"}>..</span>
            </button>
          ) : null}
          {busy ? (
            <div
              className={classNames(
                "px-4 py-4 text-sm",
                isDark ? "text-slate-400" : "text-gray-500",
              )}
            >
              Loading…
            </div>
          ) : items.length === 0 ? (
            <div
              className={classNames(
                "px-4 py-4 text-sm",
                isDark ? "text-slate-400" : "text-gray-500",
              )}
            >
              No files here.
            </div>
          ) : (
            items.map((item) => {
              const isSelected = !item.is_dir && selectedPath === item.path;
              return (
                <button
                  key={item.path}
                  type="button"
                  onClick={() => (item.is_dir ? onOpenDir(item.path) : onSelectFile(item.path))}
                  className={classNames(
                    "flex w-full items-center gap-3 px-4 py-3 text-left text-sm transition-colors",
                    isDark
                      ? "text-slate-200 hover:bg-slate-900/70"
                      : "text-gray-800 hover:bg-gray-50",
                    isSelected &&
                      (isDark
                        ? "bg-white/[0.08] text-white"
                        : "bg-[rgb(245,245,245)] text-[rgb(35,36,37)]"),
                  )}
                >
                  <span
                    className={classNames(
                      "w-5 text-center text-xs",
                      item.is_dir
                        ? isDark
                          ? "text-white"
                          : "text-[rgb(35,36,37)]"
                        : isDark
                          ? "text-slate-500"
                          : "text-gray-400",
                    )}
                  >
                    {item.is_dir ? "DIR" : "FILE"}
                  </span>
                  <span className="min-w-0 flex-1 truncate">{item.name}</span>
                  {!item.is_dir && item.mime_type ? (
                    <span
                      className={classNames(
                        "text-[11px]",
                        isDark ? "text-slate-500" : "text-gray-400",
                      )}
                    >
                      {item.mime_type}
                    </span>
                  ) : null}
                </button>
              );
            })
          )}
        </div>
      )}
    </div>
  );
}

export function PresentationPinModal({
  isOpen,
  isDark,
  groupId,
  slot,
  busy,
  onClose,
  onSubmitUrl,
  onSubmitWorkspace,
  onSubmitFile,
}: PresentationPinModalProps) {
  const { t } = useTranslation("chat");
  const { modalRef } = useModalA11y(isOpen, onClose);
  const slotId = String(slot?.slot_id || "").trim();
  const slotIndex = Number(slot?.index || 0) || 0;
  const card = slot?.card || null;
  const replaceMode = !!card;
  const defaultSource = initialSource(slot);
  const defaultWorkspaceSelection = initialWorkspacePath(slot);
  const defaultWorkspaceDir = dirname(defaultWorkspaceSelection);

  const [source, setSource] = useState<PinSource>(() => defaultSource);
  const [url, setUrl] = useState(() => String(card?.content?.url || "").trim());
  const [title, setTitle] = useState(() => String(card?.title || "").trim());
  const [summary, setSummary] = useState(() => String(card?.summary || "").trim());
  const [file, setFile] = useState<File | null>(null);
  const [error, setError] = useState("");

  const [workspaceRootPath, setWorkspaceRootPath] = useState("");
  const [workspaceCurrentPath, setWorkspaceCurrentPath] = useState(defaultWorkspaceDir);
  const [workspaceParentPath, setWorkspaceParentPath] = useState<string | null>(null);
  const [workspaceItems, setWorkspaceItems] = useState<PresentationWorkspaceItem[]>([]);
  const [workspaceSelection, setWorkspaceSelection] = useState(defaultWorkspaceSelection);
  const [workspaceBusy, setWorkspaceBusy] = useState(false);
  const [workspaceError, setWorkspaceError] = useState("");

  const currentWorkspaceLabel = useMemo(() => {
    if (!workspaceSelection) {
      return t("presentationWorkspaceSelectionEmpty", {
        defaultValue: "Choose a file from this group's workspace.",
      });
    }
    return workspaceSelection;
  }, [t, workspaceSelection]);

  const loadWorkspaceDir = useCallback(
    async (path: string) => {
      const gid = String(groupId || "").trim();
      if (!gid) return;
      setWorkspaceBusy(true);
      setWorkspaceError("");
      const resp = await fetchPresentationWorkspaceListing(gid, path);
      if (!resp.ok) {
        setWorkspaceError(`${resp.error.code}: ${resp.error.message}`);
        setWorkspaceBusy(false);
        return;
      }
      setWorkspaceRootPath(resp.result.root_path);
      setWorkspaceCurrentPath(resp.result.path);
      setWorkspaceParentPath(resp.result.parent);
      setWorkspaceItems(resp.result.items);
      setWorkspaceBusy(false);
    },
    [groupId],
  );

  useEffect(() => {
    if (!isOpen || source !== "workspace") return;
    if (workspaceRootPath || workspaceBusy) return;
    const timer = window.setTimeout(() => {
      void loadWorkspaceDir(defaultWorkspaceDir);
    }, 0);
    return () => window.clearTimeout(timer);
  }, [defaultWorkspaceDir, isOpen, loadWorkspaceDir, source, workspaceBusy, workspaceRootPath]);

  if (!isOpen || !slotId || slotIndex <= 0) return null;

  const handleSubmit = async () => {
    setError("");
    if (source === "url") {
      const trimmedUrl = String(url || "").trim();
      if (!trimmedUrl) {
        setError(t("presentationUrlRequired", { defaultValue: "Enter a URL first." }));
        return;
      }
      const normalizedUrl = normalizePresentationUrlInput(trimmedUrl);
      if (!isValidPresentationWebUrl(normalizedUrl)) {
        setError(
          t("presentationUrlInvalid", {
            defaultValue: "This does not look like a valid URL. Try example.com or localhost:3000.",
          }),
        );
        return;
      }
      await onSubmitUrl({
        slotId,
        url: normalizedUrl,
        title: String(title || "").trim(),
        summary: String(summary || "").trim(),
      });
      return;
    }
    if (source === "workspace") {
      const trimmedPath = String(workspaceSelection || "").trim();
      if (!trimmedPath) {
        setError(
          t("presentationWorkspaceRequired", { defaultValue: "Choose a workspace file first." }),
        );
        return;
      }
      await onSubmitWorkspace({
        slotId,
        path: trimmedPath,
        title: String(title || "").trim(),
        summary: String(summary || "").trim(),
      });
      return;
    }
    if (!file) {
      setError(t("presentationFileRequired", { defaultValue: "Choose a file first." }));
      return;
    }
    await onSubmitFile({
      slotId,
      file,
      title: String(title || "").trim(),
      summary: String(summary || "").trim(),
    });
  };

  return (
    <ModalFrame
      isDark={isDark}
      onClose={busy ? () => void 0 : onClose}
      titleId="presentation-pin-title"
      title={
        replaceMode
          ? t("presentationReplaceSlotTitle", {
              index: slotIndex,
              defaultValue: `Edit slot ${slotIndex}`,
            })
          : t("presentationPinSlotTitle", {
              index: slotIndex,
              defaultValue: `Pin to slot ${slotIndex}`,
            })
      }
      closeAriaLabel={t("presentationClosePinModal", {
        defaultValue: "Close presentation pin dialog",
      })}
      panelClassName="h-full w-full sm:h-auto sm:max-w-3xl"
      modalRef={modalRef}
    >
      <div className="flex min-h-0 flex-1 flex-col">
        <div
          className={classNames(
            "border-b px-5 py-4 text-sm",
            isDark ? "border-white/10 text-slate-300" : "border-black/10 text-gray-700",
          )}
        >
          <div className="font-medium">
            {replaceMode
              ? t("presentationReplaceHelp", {
                  defaultValue:
                    "Edit the current slot by updating it with a URL, a workspace file, or an uploaded snapshot.",
                })
              : t("presentationPinHelp", {
                  defaultValue:
                    "Pin a URL, a workspace file, or an uploaded snapshot so it stays visible in this group's Presentation rail.",
                })}
          </div>
          {card ? (
            <div
              className={classNames("mt-2 text-xs", isDark ? "text-slate-500" : "text-gray-500")}
            >
              {t("presentationCurrentCard", {
                title: card.title,
                defaultValue: `Current: ${card.title}`,
              })}
            </div>
          ) : null}
        </div>

        <div className="flex-1 space-y-5 overflow-auto px-5 py-5">
          <div
            className={classNames(
              "inline-flex flex-wrap rounded-full border p-1",
              isDark ? "border-white/10 bg-slate-900/60" : "border-black/10 bg-gray-100/80",
            )}
            role="tablist"
            aria-label={t("presentationPinSourceLabel", { defaultValue: "Choose a source type" })}
          >
            {(
              [
                ["url", t("presentationPinSourceUrl", { defaultValue: "URL" })],
                [
                  "workspace",
                  t("presentationPinSourceWorkspace", {
                    defaultValue: "Pick from workspace (host)",
                  }),
                ],
                [
                  "upload",
                  t("presentationPinSourceUpload", { defaultValue: "Upload from this device" }),
                ],
              ] as const
            ).map(([value, label]) => {
              const active = source === value;
              return (
                <button
                  key={value}
                  type="button"
                  onClick={() => {
                    setSource(value);
                    setError("");
                    if (value === "workspace" && !workspaceRootPath && !workspaceBusy) {
                      void loadWorkspaceDir(defaultWorkspaceDir);
                    }
                  }}
                  className={classNames(
                    "rounded-full px-4 py-2 text-sm font-medium transition-colors",
                    active
                      ? isDark
                        ? "border border-white/12 bg-white/[0.08] text-white shadow-sm"
                        : "border border-black/10 bg-[rgb(245,245,245)] text-[rgb(35,36,37)] shadow-sm"
                      : isDark
                        ? "text-slate-300 hover:bg-slate-800/70"
                        : "text-gray-700 hover:bg-white",
                  )}
                  aria-pressed={active}
                >
                  {label}
                </button>
              );
            })}
          </div>

          {source === "url" ? (
            <label className="block space-y-2">
              <span
                className={classNames(
                  "text-sm font-medium",
                  isDark ? "text-slate-200" : "text-gray-900",
                )}
              >
                {t("presentationUrlLabel", { defaultValue: "URL" })}
              </span>
              <input
                type="url"
                value={url}
                onChange={(event) => setUrl(event.target.value)}
                placeholder={t("presentationUrlPlaceholder", {
                  defaultValue: "https://example.com/report",
                })}
                className={classNames(
                  "w-full rounded-2xl border px-4 py-3 text-sm outline-none transition-colors",
                  isDark
                    ? "border-white/10 bg-slate-950/70 text-slate-100 placeholder:text-slate-500 focus:border-white/30"
                    : "border-black/10 bg-white text-gray-900 placeholder:text-gray-400 focus:border-black/20",
                )}
              />
            </label>
          ) : null}

          {source === "workspace" ? (
            <div className="space-y-3">
              <div
                className={classNames(
                  "text-xs leading-5",
                  isDark ? "text-slate-400" : "text-gray-600",
                )}
              >
                {t("presentationWorkspaceHint", {
                  defaultValue:
                    "Link a file from this group's active workspace. Updates to that file will show up here without re-pinning.",
                })}
              </div>
              <WorkspaceList
                isDark={isDark}
                rootPath={workspaceRootPath}
                currentPath={workspaceCurrentPath}
                parentPath={workspaceParentPath}
                items={workspaceItems}
                selectedPath={workspaceSelection}
                busy={workspaceBusy}
                error={workspaceError}
                onOpenDir={(path) => void loadWorkspaceDir(path)}
                onSelectFile={(path) => {
                  setWorkspaceSelection(path);
                  setError("");
                }}
              />
              <div className="space-y-2">
                <span
                  className={classNames(
                    "text-sm font-medium",
                    isDark ? "text-slate-200" : "text-gray-900",
                  )}
                >
                  {t("presentationWorkspaceSelectionLabel", { defaultValue: "Selected file" })}
                </span>
                <div
                  className={classNames(
                    "rounded-2xl border px-4 py-3 text-sm font-mono",
                    isDark
                      ? "border-white/10 bg-slate-950/70 text-slate-100"
                      : "border-black/10 bg-white text-gray-900",
                  )}
                >
                  {currentWorkspaceLabel}
                </div>
              </div>
            </div>
          ) : null}

          {source === "upload" ? (
            <label className="block space-y-2">
              <span
                className={classNames(
                  "text-sm font-medium",
                  isDark ? "text-slate-200" : "text-gray-900",
                )}
              >
                {t("presentationFileLabel", { defaultValue: "Upload from this device" })}
              </span>
              <input
                type="file"
                onChange={(event) => setFile(event.target.files?.[0] || null)}
                className={classNames(
                  "block w-full rounded-2xl border px-4 py-3 text-sm file:mr-4 file:rounded-full file:border-0 file:px-3 file:py-2 file:text-sm file:font-medium",
                  isDark
                    ? "border-white/10 bg-slate-950/70 text-slate-100 file:bg-slate-800 file:text-slate-100"
                    : "border-black/10 bg-white text-gray-900 file:bg-gray-100 file:text-gray-900",
                )}
              />
              <div className={classNames("text-xs", isDark ? "text-slate-500" : "text-gray-500")}>
                {file
                  ? file.name
                  : t("presentationFileHint", {
                      defaultValue:
                        "Uploads are stored as a snapshot. HTML, PDF, images, Markdown, and regular files are all supported.",
                    })}
              </div>
            </label>
          ) : null}

          <label className="block space-y-2">
            <span
              className={classNames(
                "text-sm font-medium",
                isDark ? "text-slate-200" : "text-gray-900",
              )}
            >
              {t("presentationTitleLabel", { defaultValue: "Title" })}
            </span>
            <input
              type="text"
              value={title}
              onChange={(event) => setTitle(event.target.value)}
              placeholder={t("presentationTitlePlaceholder", {
                defaultValue: "Optional title override",
              })}
              className={classNames(
                "w-full rounded-2xl border px-4 py-3 text-sm outline-none transition-colors",
                isDark
                  ? "border-white/10 bg-slate-950/70 text-slate-100 placeholder:text-slate-500 focus:border-white/30"
                  : "border-black/10 bg-white text-gray-900 placeholder:text-gray-400 focus:border-black/20",
              )}
            />
          </label>

          <label className="block space-y-2">
            <span
              className={classNames(
                "text-sm font-medium",
                isDark ? "text-slate-200" : "text-gray-900",
              )}
            >
              {t("presentationSummaryLabel", { defaultValue: "Summary" })}
            </span>
            <textarea
              value={summary}
              onChange={(event) => setSummary(event.target.value)}
              rows={3}
              placeholder={t("presentationSummaryPlaceholder", {
                defaultValue: "Optional summary shown in the slot preview",
              })}
              className={classNames(
                "w-full rounded-2xl border px-4 py-3 text-sm outline-none transition-colors",
                isDark
                  ? "border-white/10 bg-slate-950/70 text-slate-100 placeholder:text-slate-500 focus:border-white/30"
                  : "border-black/10 bg-white text-gray-900 placeholder:text-gray-400 focus:border-black/20",
              )}
            />
          </label>

          {error ? (
            <div className={classNames("text-sm", isDark ? "text-rose-300" : "text-rose-600")}>
              {error}
            </div>
          ) : null}
        </div>

        <div
          className={classNames(
            "flex items-center justify-end gap-3 border-t px-5 py-4",
            isDark ? "border-white/10" : "border-black/10",
          )}
        >
          <button
            type="button"
            onClick={onClose}
            disabled={busy}
            className={classNames(
              "rounded-full px-4 py-2 text-sm font-medium transition-colors",
              isDark ? "text-slate-300 hover:bg-slate-800/70" : "text-gray-700 hover:bg-gray-100",
              busy && "cursor-not-allowed opacity-60",
            )}
          >
            {t("presentationCancelAction", { defaultValue: "Cancel" })}
          </button>
          <button
            type="button"
            onClick={() => void handleSubmit()}
            disabled={busy}
            className={classNames(
              "rounded-full px-4 py-2 text-sm font-medium text-white transition-colors",
              busy
                ? "bg-black/40 dark:bg-white/40"
                : "border border-[rgb(35,36,37)] bg-[rgb(35,36,37)] hover:bg-black dark:border-white dark:bg-white dark:text-[rgb(35,36,37)] dark:hover:bg-white/92",
            )}
          >
            {replaceMode
              ? t("presentationReplaceSubmit", { defaultValue: "Save changes" })
              : t("presentationPinSubmit", { defaultValue: "Pin to slot" })}
          </button>
        </div>
      </div>
    </ModalFrame>
  );
}
