import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import type { WorkspaceFile } from "../../types";
import { workspaceContentUrl } from "../../services/api/workspace";
import { LazyMarkdownRenderer } from "../LazyMarkdownRenderer";
import { parseWorkspaceTable, workspaceLink } from "./workspacePreview";

import { staticWorkspaceHtml } from "./workspaceHtml";
import { scrollWorkspaceFragment } from "./workspaceFragment";
import type { WorkspaceFileNavigation, WorkspaceOpenFileOptions } from "./useWorkspaceEditor";

export function WorkspaceDocumentPreview({
  kind,
  groupId,
  file,
  content,
  isDark,
  onOpenFile,
  navigation,
}: {
  kind: "markdown" | "html" | "csv" | "tsv";
  groupId: string;
  file: WorkspaceFile;
  content: string;
  isDark: boolean;
  onOpenFile?: (path: string, options?: WorkspaceOpenFileOptions) => void;
  navigation?: WorkspaceFileNavigation | null;
}) {
  const { t } = useTranslation("chat");
  const viewport = useRef<HTMLDivElement>(null);
  const appliedNavigation = useRef<WorkspaceFileNavigation | null>(null);
  const [loadedHtml, setLoadedHtml] = useState<{ source: string; document: Document } | null>(null);
  const applyNavigation = useCallback(
    (root: Document | HTMLElement) => {
      if (!navigation || appliedNavigation.current === navigation) return;
      appliedNavigation.current = navigation;
      scrollWorkspaceFragment(root, navigation.fragment);
    },
    [navigation],
  );
  const markdownRendered = useCallback(() => {
    if (viewport.current) applyNavigation(viewport.current);
  }, [applyNavigation]);
  const contentUrl = workspaceContentUrl(groupId, file);
  const endpoint = new URL(contentUrl, window.location.href);
  const resourceEndpoint = endpoint.origin + endpoint.pathname;
  const resolveUrl = useCallback(
    (href: string, target: "image" | "link") => {
      if (href.startsWith("#")) return target === "link" ? href : "";
      if (/^https?:\/\//i.test(href)) return href;
      if (target === "link" && /^mailto:/i.test(href)) return href;
      if (target === "image" && /^data:image\/(png|gif|jpeg|webp|svg\+xml)[;,]/i.test(href))
        return href;
      const local = workspaceLink(file.path, href);
      return local ? workspaceContentUrl(groupId, { ...file, path: local.path }) + local.hash : "";
    },
    [groupId, file],
  );
  const html = useMemo(
    () => (kind === "html" ? staticWorkspaceHtml(content, resolveUrl, resourceEndpoint) : ""),
    [kind, content, resolveUrl, resourceEndpoint],
  );
  const table = useMemo(
    () =>
      kind === "csv" || kind === "tsv"
        ? parseWorkspaceTable(content, kind === "csv" ? "," : "\t")
        : null,
    [kind, content],
  );

  const followLink = useCallback(
    (
      event: { target: EventTarget | null; preventDefault: () => void },
      doc: Document | HTMLElement,
    ) => {
      const anchor = (event.target as Element | null)?.closest("a[href], area[href]");
      if (!anchor) return;
      event.preventDefault();
      const href = anchor.getAttribute("href") || "";
      if (!href) return;
      if (href.startsWith("#")) {
        if (onOpenFile) onOpenFile(file.path, { fragment: href });
        else scrollWorkspaceFragment(doc, href);
        return;
      }
      const url = new URL(href, window.location.href);
      if (
        url.origin + url.pathname === resourceEndpoint &&
        url.searchParams.get("scope_key") === file.scope_key &&
        url.searchParams.get("scope_url") === file.scope_url
      ) {
        const path = url.searchParams.get("path");
        const fragment = url.hash || (href.endsWith("#") ? "#" : "");
        if (path) {
          if (fragment) onOpenFile?.(path, { fragment });
          else onOpenFile?.(path);
        }
      } else if (["https:", "http:", "mailto:"].includes(url.protocol)) {
        window.open(url.href, "_blank", "noopener,noreferrer");
      }
    },
    [file, onOpenFile, resourceEndpoint],
  );

  useEffect(() => {
    if (kind !== "html" || !loadedHtml || loadedHtml.source !== html) return;
    const doc = loadedHtml.document;
    const click = (event: MouseEvent) => followLink(event, doc);
    doc.addEventListener("click", click);
    applyNavigation(doc);
    return () => doc.removeEventListener("click", click);
  }, [kind, loadedHtml, html, followLink, applyNavigation]);

  if (kind === "html")
    return (
      <div className="flex min-h-0 flex-1 flex-col">
        <p className="shrink-0 border-b border-[var(--glass-border-subtle)] px-3 py-2 text-xs text-[var(--color-text-secondary)]">
          {t("workspaceHtmlStatic")}
        </p>
        <iframe
          title={file.path}
          srcDoc={html}
          sandbox="allow-same-origin"
          className="min-h-0 w-full flex-1 border-0 bg-white"
          onLoad={(event) => {
            const doc = event.currentTarget.contentDocument;
            if (doc) setLoadedHtml({ source: html, document: doc });
          }}
        />
      </div>
    );
  if (table)
    return (
      <div
        className="min-h-0 flex-1 overflow-auto p-3"
        tabIndex={0}
        aria-label={t("workspaceTablePreview")}
      >
        {table.error ? (
          <p role="alert">{t("workspaceTableInvalid")}</p>
        ) : (
          <>
            {table.limited && (
              <p role="status" className="mb-2 text-xs text-[var(--color-text-secondary)]">
                {t("workspaceTableLimited", {
                  rows: Math.min(table.rowCount, 200),
                  columns: Math.min(table.columns, 50),
                })}
              </p>
            )}
            <table className="min-w-full border-collapse text-left text-xs">
              <tbody>
                {table.rows.map((row, rowIndex) => (
                  <tr key={rowIndex}>
                    <th
                      scope="row"
                      className="border border-[var(--glass-border-subtle)] px-2 py-1 font-normal text-[var(--color-text-tertiary)]"
                    >
                      {rowIndex + 1}
                    </th>
                    {row.map((cell, columnIndex) => (
                      <td
                        key={columnIndex}
                        className="max-w-lg whitespace-pre-wrap break-words border border-[var(--glass-border-subtle)] px-2 py-1 align-top"
                      >
                        {cell}
                      </td>
                    ))}
                  </tr>
                ))}
              </tbody>
            </table>
          </>
        )}
      </div>
    );
  return (
    <div
      ref={viewport}
      className="min-h-0 flex-1 overflow-auto p-4"
      onClick={(event) => followLink(event, event.currentTarget)}
    >
      <LazyMarkdownRenderer
        content={content}
        isDark={isDark}
        enableMermaid
        resolveUrl={resolveUrl}
        onRendered={markdownRendered}
        className="break-words [overflow-wrap:anywhere]"
        fallback={<pre className="whitespace-pre-wrap">{content}</pre>}
      />
    </div>
  );
}
