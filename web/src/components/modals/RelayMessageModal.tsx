import { useEffect, useMemo, useState } from "react";
import { useTranslation } from "react-i18next";
import type { Actor, ChatMessageData, GroupMeta, LedgerEvent } from "../../types";
import { classNames } from "../../utils/classNames";
import * as api from "../../services/api";
import { useModalA11y } from "../../hooks/useModalA11y";

export interface RelayMessageModalProps {
  isOpen: boolean;
  isDark: boolean;
  busy: boolean;
  srcGroupId: string;
  srcEvent: LedgerEvent | null;
  groups: GroupMeta[];
  onCancel: () => void;
  onSubmit: (dstGroupId: string, to: string[], note: string) => void;
}

export function RelayMessageModal({
  isOpen,
  isDark,
  busy,
  srcGroupId,
  srcEvent,
  groups,
  onCancel,
  onSubmit,
}: RelayMessageModalProps) {
  const { t } = useTranslation("modals");
  const dstGroups = useMemo(() => {
    return (groups || [])
      .map((g) => ({ group_id: String(g.group_id || ""), title: String(g.title || "") }))
      .filter((g) => g.group_id && g.group_id !== srcGroupId);
  }, [groups, srcGroupId]);

  const defaultDstGroupId = useMemo(() => {
    return dstGroups[0]?.group_id || "";
  }, [dstGroups]);

  const [dstGroupId, setDstGroupId] = useState(() => defaultDstGroupId);
  const [dstActors, setDstActors] = useState<Actor[]>([]);
  const [dstActorsBusy, setDstActorsBusy] = useState(() => !!defaultDstGroupId);
  const [note, setNote] = useState("");
  const [toTokens, setToTokens] = useState<string[]>(["@all"]);

  const srcEventId = srcEvent?.id ? String(srcEvent.id) : "";
  const srcBy = srcEvent?.by ? String(srcEvent.by) : "";
  const srcText = useMemo(() => {
    const d = srcEvent?.data as ChatMessageData | undefined;
    const t = d?.text;
    return typeof t === "string" ? t : "";
  }, [srcEvent]);

  useEffect(() => {
    if (!isOpen) return;
    const gid = String(dstGroupId || "").trim();
    if (!gid) return;
    let cancelled = false;
    void api
      .fetchActors(gid)
      .then((resp) => {
        if (cancelled) return;
        if (!resp.ok) {
          setDstActors([]);
          return;
        }
        setDstActors(resp.result.actors || []);
      })
      .finally(() => {
        if (cancelled) return;
        setDstActorsBusy(false);
      });
    return () => {
      cancelled = true;
    };
  }, [dstGroupId, isOpen]);

  const availableTokens = useMemo(() => {
    const base = ["@all", "@foreman", "@peers"];
    const actorIds = (dstActors || []).map((a) => String(a.id || "")).filter((id) => id);
    actorIds.sort();
    return [...base, ...actorIds];
  }, [dstActors]);

  const toggleToken = (token: string) => {
    const t = token.trim();
    if (!t) return;
    setToTokens((prev) => {
      const exists = prev.includes(t);
      if (exists) return prev.filter((x) => x !== t);
      return prev.concat([t]);
    });
  };

  const { modalRef } = useModalA11y(isOpen, onCancel);

  if (!isOpen) return null;

  const canSubmit = !!dstGroupId && !!srcEventId && !!(srcText || note).trim() && !busy;

  return (
    <div
      className={`fixed inset-0 backdrop-blur-sm flex items-stretch sm:items-start justify-center p-0 sm:p-6 z-50 animate-fade-in ${
        isDark ? "bg-black/50" : "bg-black/30"
      }`}
      onPointerDown={(e) => {
        if (e.target === e.currentTarget) onCancel();
      }}
      role="dialog"
      aria-modal="true"
      aria-labelledby="relay-modal-title"
    >
      <div
        ref={modalRef}
        className={classNames(
          "w-full h-full sm:h-auto sm:max-w-2xl sm:mt-14 border shadow-2xl animate-scale-in overflow-hidden flex flex-col rounded-none sm:rounded-2xl",
          isDark ? "border-slate-700/50 bg-gradient-to-b from-slate-800 to-slate-900" : "border-gray-200 bg-white"
        )}
      >
        <div className={classNames("px-6 py-4 border-b safe-area-inset-top", isDark ? "border-slate-700/50" : "border-gray-200")}>
          <div id="relay-modal-title" className={classNames("text-lg font-semibold", isDark ? "text-white" : "text-gray-900")}>
            {t("relay.title")}
          </div>
          <div className={classNames("text-xs mt-1", isDark ? "text-slate-400" : "text-gray-500")}>
            {t("relay.subtitle")}
          </div>
        </div>

        <div className="p-6 space-y-5 flex-1 overflow-y-auto min-h-0">
          {/* Source preview */}
          <div className={classNames("rounded-xl border p-4", isDark ? "border-white/10 bg-slate-900/40" : "border-black/10 bg-gray-50")}>
            <div className={classNames("text-xs font-semibold", isDark ? "text-slate-200" : "text-gray-800")}>
              {t("relay.source")}
            </div>
            <div className={classNames("text-[11px] mt-1", isDark ? "text-slate-400" : "text-gray-600")}>
              {srcGroupId} · {srcEventId ? srcEventId : "—"} · {srcBy || "—"}
            </div>
            <div className={classNames("mt-2 text-sm whitespace-pre-wrap break-words", isDark ? "text-slate-100" : "text-gray-800")}>
              {srcText || t("relay.emptyMessage")}
            </div>
          </div>

          {/* Destination group */}
          <div>
            <label className={classNames("block text-xs font-medium mb-2", isDark ? "text-slate-300" : "text-gray-700")}>
              {t("relay.destinationGroup")}
            </label>
            <select
              value={dstGroupId}
              onChange={(e) => {
                const gid = e.target.value;
                setDstGroupId(gid);
                setDstActors([]);
                setDstActorsBusy(true);
              }}
              className={classNames(
                "w-full rounded-xl border px-3 py-2.5 text-sm min-h-[44px] transition-colors",
                isDark ? "bg-slate-900/80 border-slate-600/50 text-white" : "bg-white border-gray-300 text-gray-900"
              )}
              disabled={busy || dstGroups.length === 0}
            >
              {dstGroups.length === 0 ? (
                <option value="">{t("relay.noOtherGroups")}</option>
              ) : null}
              {dstGroups.map((g) => (
                <option key={g.group_id} value={g.group_id}>
                  {g.title ? `${g.title} (${g.group_id})` : g.group_id}
                </option>
              ))}
            </select>
          </div>

          {/* Recipients */}
          <div>
            <div className="flex items-center justify-between gap-3">
              <label className={classNames("block text-xs font-medium", isDark ? "text-slate-300" : "text-gray-700")}>
                {t("relay.recipients")}
              </label>
              <button
                type="button"
                className={classNames("text-xs underline", isDark ? "text-slate-400 hover:text-slate-200" : "text-gray-500 hover:text-gray-800")}
                onClick={() => setToTokens([])}
                disabled={busy}
              >
                {t("common:reset")}
              </button>
            </div>

            <div className={classNames("mt-2 flex flex-wrap gap-2", dstActorsBusy ? "opacity-60" : "")}>
              {availableTokens.map((tok) => {
                const active = toTokens.includes(tok);
                return (
                  <button
                    key={tok}
                    type="button"
                    className={classNames(
                      "text-xs px-2.5 py-1.5 rounded-full border transition-colors",
                      active
                        ? isDark
                          ? "bg-blue-600/40 border-blue-500/40 text-blue-100"
                          : "bg-blue-50 border-blue-200 text-blue-700"
                        : isDark
                          ? "bg-slate-900/30 border-white/10 text-slate-300 hover:bg-slate-900/50"
                          : "bg-white border-gray-200 text-gray-700 hover:bg-gray-50"
                    )}
                    onClick={() => toggleToken(tok)}
                    disabled={busy}
                    title={tok}
                  >
                    {tok}
                  </button>
                );
              })}
            </div>
            <div className={classNames("mt-2 text-xs", isDark ? "text-slate-500" : "text-gray-500")}>
              {toTokens.length ? t("relay.selectedTokens", { tokens: toTokens.join(", ") }) : t("relay.selectedBroadcast")}
            </div>
          </div>

          {/* Optional note */}
          <div>
            <label className={classNames("block text-xs font-medium mb-2", isDark ? "text-slate-300" : "text-gray-700")}>
              {t("relay.noteLabel")}
            </label>
            <textarea
              value={note}
              onChange={(e) => setNote(e.target.value)}
              rows={3}
              className={classNames(
                "w-full rounded-xl border px-3 py-2.5 text-sm transition-colors",
                isDark ? "bg-slate-900/80 border-slate-600/50 text-white" : "bg-white border-gray-300 text-gray-900"
              )}
              placeholder={t("relay.notePlaceholder")}
              disabled={busy}
            />
          </div>

          <div className="flex gap-3 pt-1 flex-wrap justify-end">
            <button
              className={classNames(
                "px-4 py-2.5 rounded-xl text-sm font-medium transition-colors min-h-[44px]",
                isDark ? "bg-slate-700 hover:bg-slate-600 text-slate-200" : "bg-gray-100 hover:bg-gray-200 text-gray-700"
              )}
              onClick={onCancel}
              disabled={busy}
            >
              {t("common:cancel")}
            </button>
            <button
              className="px-4 py-2.5 rounded-xl bg-blue-600 hover:bg-blue-500 text-white text-sm font-semibold shadow-lg disabled:opacity-50 transition-all min-h-[44px]"
              onClick={() => onSubmit(dstGroupId, toTokens, note)}
              disabled={!canSubmit}
            >
              {busy ? t("relay.sending") : t("relay.relay")}
            </button>
          </div>
        </div>
      </div>
    </div>
  );
}
