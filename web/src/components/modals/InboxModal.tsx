import { useMemo } from "react";
import { useTranslation } from "react-i18next";
import { Actor, LedgerEvent } from "../../types";
import { formatFullTime, formatTime } from "../../utils/time";
import { MarkdownRenderer } from "../MarkdownRenderer";
import { useModalA11y } from "../../hooks/useModalA11y";

function formatEventLine(
  ev: LedgerEvent,
  getDisplayName: (id: string) => string
): string {
  if (ev.kind === "chat.message" && ev.data && typeof ev.data === "object") {
    const d = ev.data as Record<string, unknown>;
    return typeof d.text === "string" ? d.text : "";
  }
  if (ev.kind === "system.notify" && ev.data && typeof ev.data === "object") {
    const d = ev.data as Record<string, unknown>;
    const kind = typeof d.kind === "string" ? d.kind : "info";
    const title = typeof d.title === "string" ? d.title : "";
    const message = typeof d.message === "string" ? d.message : "";
    const targetId = typeof d.target_actor_id === "string" ? d.target_actor_id : "";
    const target = targetId ? ` → ${getDisplayName(targetId)}` : "";
    return `[${kind}]${target}: ${title}${message ? ` - ${message}` : ""}`;
  }
  const k = String(ev.kind || "event");
  const byId = ev.by ? String(ev.by) : "";
  const by = byId ? ` by ${getDisplayName(byId)}` : "";
  return `${k}${by}`;
}

export interface InboxModalProps {
  isOpen: boolean;
  isDark: boolean;
  actorId: string;
  actors: Actor[];
  messages: LedgerEvent[];
  busy: string;
  onClose: () => void;
  onMarkAllRead: () => void;
}

export function InboxModal({ isOpen, isDark, actorId, actors, messages, busy, onClose, onMarkAllRead }: InboxModalProps) {
  const { t } = useTranslation("modals");
  const { modalRef } = useModalA11y(isOpen, onClose);
  // Helper to get display name for actor
  const getDisplayName = useMemo(() => {
    const map = new Map<string, string>();
    for (const actor of actors) {
      const id = String(actor.id || "");
      if (id) map.set(id, actor.title || id);
    }
    return (id: string) => {
      if (!id || id === "user") return id;
      return map.get(id) || id;
    };
  }, [actors]);

  if (!isOpen) return null;

  return (
    <div
      className={`fixed inset-0 backdrop-blur-sm flex items-stretch sm:items-start justify-center p-0 sm:p-6 z-50 animate-fade-in ${isDark ? "bg-black/50" : "bg-black/30"}`}
      onMouseDown={(e) => {
        if (e.target === e.currentTarget) onClose();
      }}
      role="dialog"
      aria-modal="true"
      aria-labelledby="inbox-title"
    >
      <div
        ref={modalRef}
        className={`w-full h-full sm:h-auto sm:max-h-[calc(100dvh-8rem)] sm:max-w-2xl sm:mt-16 border shadow-2xl animate-scale-in flex flex-col rounded-none sm:rounded-2xl ${isDark ? "border-slate-700/50 bg-gradient-to-b from-slate-800 to-slate-900" : "border-gray-200 bg-white"
          }`}
      >
        <div className={`px-4 sm:px-6 py-4 border-b flex items-center justify-between gap-3 safe-area-inset-top ${isDark ? "border-slate-700/50" : "border-gray-200"}`}>
          <div className="min-w-0">
            <div id="inbox-title" className={`text-lg font-semibold truncate ${isDark ? "text-white" : "text-gray-900"}`}>
              {t("inbox.title", { actorId })}
            </div>
            <div className={`text-sm ${isDark ? "text-slate-400" : "text-gray-500"}`}>{t("inbox.unreadMessages", { count: messages.length })}</div>
          </div>
          <div className="flex gap-2">
            <button
              className={`rounded-xl px-4 py-2 text-sm font-medium disabled:opacity-50 transition-colors min-h-[44px] ${isDark ? "bg-slate-700 hover:bg-slate-600 text-slate-200" : "bg-gray-100 hover:bg-gray-200 text-gray-700"
                }`}
              onClick={onMarkAllRead}
              disabled={!messages.length || busy.startsWith("inbox")}
            >
              {t("inbox.markAllRead")}
            </button>
            <button
              className={`rounded-xl px-4 py-2 text-sm font-medium transition-colors min-h-[44px] ${isDark ? "bg-slate-600 hover:bg-slate-500 text-white" : "bg-gray-200 hover:bg-gray-300 text-gray-800"
                }`}
              onClick={onClose}
            >
              {t("common:close")}
            </button>
          </div>
        </div>

        <div className="flex-1 min-h-0 overflow-auto p-4 space-y-2">
          {messages.map((ev, idx) => (
            <div
              key={String(ev.id || idx)}
              className={`rounded-xl border px-4 py-3 ${isDark ? "border-slate-700/50 bg-slate-800/50" : "border-gray-200 bg-gray-50"}`}
            >
              <div className="flex items-center justify-between gap-3">
                <div className={`text-xs truncate ${isDark ? "text-slate-400" : "text-gray-500"}`} title={formatFullTime(ev.ts)}>
                  {formatTime(ev.ts)}
                </div>
                <div className={`text-xs font-medium truncate ${isDark ? "text-slate-300" : "text-gray-700"}`}>{getDisplayName(ev.by || "") || "—"}</div>
              </div>
              <div className="mt-2 text-sm break-words">
                <MarkdownRenderer
                  content={formatEventLine(ev, getDisplayName)}
                  isDark={isDark}
                  className={isDark ? "text-slate-200" : "text-gray-800"}
                />
              </div>
            </div>
          ))}
          {!messages.length && (
            <div className="text-center py-8">
              <div className="text-3xl mb-2">📭</div>
              <div className={`text-sm ${isDark ? "text-slate-400" : "text-gray-500"}`}>{t("inbox.noUnread")}</div>
            </div>
          )}
        </div>
      </div>
    </div>
  );
}
