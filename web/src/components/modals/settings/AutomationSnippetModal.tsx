import React from "react";

import { AUTOMATION_VAR_HELP } from "./automationUtils";
import { cardClass, inputClass } from "./types";

interface AutomationSnippetModalProps {
  open: boolean;
  isDark: boolean;
  templateErr: string;
  newSnippetId: string;
  supportedVars: string[];
  snippetIds: string[];
  snippets: Record<string, string>;
  onClose: () => void;
  onNewSnippetIdChange: (next: string) => void;
  onAddSnippet: () => void;
  onDeleteSnippet: (snippetId: string) => void;
  onUpdateSnippet: (snippetId: string, content: string) => void;
}

export function AutomationSnippetModal(props: AutomationSnippetModalProps) {
  const {
    open,
    isDark,
    templateErr,
    newSnippetId,
    supportedVars,
    snippetIds,
    snippets,
    onClose,
    onNewSnippetIdChange,
    onAddSnippet,
    onDeleteSnippet,
    onUpdateSnippet,
  } = props;

  if (!open) return null;

  return (
    <div
      className="fixed inset-0 z-[1000]"
      role="dialog"
      aria-modal="true"
      onMouseDown={(e) => {
        if (e.target === e.currentTarget) onClose();
      }}
    >
      <div className="absolute inset-0 bg-black/50" />
      <div
        className={`absolute inset-2 sm:inset-auto sm:left-1/2 sm:top-1/2 sm:w-[min(820px,calc(100vw-20px))] sm:h-[min(74vh,700px)] sm:-translate-x-1/2 sm:-translate-y-1/2 rounded-xl sm:rounded-2xl border ${
          isDark ? "border-slate-800 bg-slate-950" : "border-gray-200 bg-white"
        } shadow-2xl flex flex-col overflow-hidden`}
      >
        <div className={`px-4 py-3 border-b ${isDark ? "border-slate-800" : "border-gray-200"} flex items-start gap-3`}>
          <div className="min-w-0">
            <div className={`text-sm font-semibold ${isDark ? "text-slate-100" : "text-gray-900"}`}>Snippets</div>
            <div className={`mt-1 text-[11px] ${isDark ? "text-slate-400" : "text-gray-600"}`}>
              Reusable notification messages for automation rules.
            </div>
          </div>
          <button
            type="button"
            className={`ml-auto px-3 py-2 rounded-lg text-sm min-h-[44px] transition-colors ${
              isDark ? "bg-slate-900 hover:bg-slate-800 text-slate-300 border border-slate-800" : "bg-white hover:bg-gray-50 text-gray-700 border border-gray-200"
            }`}
            onClick={onClose}
          >
            Close
          </button>
        </div>

        <div className="p-3 sm:p-4 flex-1 overflow-auto space-y-3">
          {templateErr ? <div className={`text-xs ${isDark ? "text-rose-300" : "text-rose-600"}`}>{templateErr}</div> : null}
          <div className="grid grid-cols-1 sm:grid-cols-[1fr_auto] gap-2">
            <input
              value={newSnippetId}
              onChange={(e) => onNewSnippetIdChange(e.target.value)}
              className={`${inputClass(isDark)} font-mono`}
              placeholder="snippet_name"
              spellCheck={false}
            />
            <button
              type="button"
              className={`px-3 py-2 rounded-lg text-sm min-h-[44px] font-medium transition-colors ${
                isDark ? "bg-slate-800 hover:bg-slate-700 text-slate-200 border border-slate-700" : "bg-white hover:bg-gray-50 text-gray-800 border border-gray-200"
              }`}
              onClick={onAddSnippet}
            >
              + Add Snippet
            </button>
          </div>

          {supportedVars.length > 0 ? (
            <div className={`rounded-lg border p-2.5 text-[11px] ${isDark ? "border-slate-800 bg-slate-900/60 text-slate-400" : "border-gray-200 bg-gray-50 text-gray-600"}`}>
              <div className={`font-semibold mb-1 ${isDark ? "text-slate-300" : "text-gray-700"}`}>Available placeholders</div>
              <div className="space-y-1">
                {supportedVars.map((v) => {
                  const help = AUTOMATION_VAR_HELP[v];
                  return (
                    <div key={v}>
                      <span className="font-mono">{`{{${v}}}`}</span>
                      <span>{` - ${help?.description || "Built-in placeholder."}`}</span>
                      <span className={isDark ? "text-slate-500" : "text-gray-500"}>{` (example: ${help?.example || "-"})`}</span>
                    </div>
                  );
                })}
              </div>
            </div>
          ) : null}

          {snippetIds.length === 0 ? <div className={`text-sm ${isDark ? "text-slate-400" : "text-gray-600"}`}>No snippets yet.</div> : null}

          <div className="space-y-3">
            {snippetIds.map((snippetId) => (
              <div key={snippetId} className={cardClass(isDark)}>
                <div className="flex items-center justify-between gap-2 mb-2">
                  <div className={`text-xs font-semibold font-mono ${isDark ? "text-slate-200" : "text-gray-800"}`}>{snippetId}</div>
                  <button
                    type="button"
                    className={`px-2 py-1.5 rounded-lg text-xs min-h-[36px] transition-colors ${
                      isDark ? "bg-slate-900 hover:bg-slate-800 text-slate-300 border border-slate-800" : "bg-white hover:bg-gray-50 text-gray-700 border border-gray-200"
                    }`}
                    onClick={() => onDeleteSnippet(snippetId)}
                    title="Delete snippet"
                  >
                    Delete
                  </button>
                </div>
                <textarea
                  value={snippets[snippetId] || ""}
                  onChange={(e) => onUpdateSnippet(snippetId, e.target.value)}
                  className={`${inputClass(isDark)} font-mono text-[12px]`}
                  style={{ minHeight: 140 }}
                  spellCheck={false}
                />
              </div>
            ))}
          </div>
        </div>
      </div>
    </div>
  );
}
