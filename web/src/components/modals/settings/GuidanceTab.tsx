import { useEffect, useState } from "react";
import { useTranslation, Trans } from "react-i18next";
import { apiJson } from "../../../services/api";
import { cardClass, inputClass, labelClass, primaryButtonClass, preClass } from "./types";

type PromptKind = "preamble" | "help";

type PromptInfo = {
  kind: PromptKind;
  source: "home" | "builtin";
  filename: string;
  path?: string | null;
  content: string;
};

type PromptsResponse = {
  preamble: PromptInfo;
  help: PromptInfo;
};

export function GuidanceTab({ isDark, groupId }: { isDark: boolean; groupId?: string }) {
  const { t } = useTranslation("settings");
  const [busy, setBusy] = useState(false);
  const [err, setErr] = useState("");
  const [prompts, setPrompts] = useState<Record<PromptKind, PromptInfo> | null>(null);
  const [expandedKind, setExpandedKind] = useState<PromptKind | null>(null);

  const load = async () => {
    if (!groupId) return;
    setBusy(true);
    setErr("");
    try {
      const resp = await apiJson<PromptsResponse>(`/api/v1/groups/${encodeURIComponent(groupId)}/prompts`);
      if (!resp.ok) {
        setErr(resp.error?.message || t("guidance.failedToLoad"));
        setPrompts(null);
        return;
      }
      const p = resp.result?.preamble;
      const h = resp.result?.help;
      if (!p || !h) {
        setErr(t("guidance.invalidResponse"));
        setPrompts(null);
        return;
      }
      setPrompts({ preamble: p, help: h });
    } catch {
      setErr(t("guidance.failedToLoad"));
      setPrompts(null);
    } finally {
      setBusy(false);
    }
  };

  useEffect(() => {
    if (groupId) load();
    // eslint-disable-next-line react-hooks/exhaustive-deps -- Load when groupId changes.
  }, [groupId]);

  const save = async (kind: PromptKind) => {
    if (!groupId || !prompts) return;
    setBusy(true);
    setErr("");
    try {
      const body = { content: prompts[kind].content, by: "user" };
      const resp = await apiJson<PromptInfo>(`/api/v1/groups/${encodeURIComponent(groupId)}/prompts/${kind}`, {
        method: "PUT",
        body: JSON.stringify(body),
      });
      if (!resp.ok) {
        setErr(resp.error?.message || t("guidance.failedToSave", { kind }));
        return;
      }
      await load();
    } catch {
      setErr(t("guidance.failedToSave", { kind }));
    } finally {
      setBusy(false);
    }
  };

  const reset = async (kind: PromptKind) => {
    if (!groupId) return;
    const filename = prompts?.[kind]?.filename || kind;
    const ok = window.confirm(t("automation.resetGuidanceConfirm", { kind, filename }));
    if (!ok) return;

    setBusy(true);
    setErr("");
    try {
      const resp = await apiJson<PromptInfo>(
        `/api/v1/groups/${encodeURIComponent(groupId)}/prompts/${kind}?confirm=${encodeURIComponent(kind)}`,
        { method: "DELETE" }
      );
      if (!resp.ok) {
        setErr(resp.error?.message || t("guidance.failedToReset", { kind }));
        return;
      }
      await load();
    } catch {
      setErr(t("guidance.failedToReset", { kind }));
    } finally {
      setBusy(false);
    }
  };

  const setContent = (kind: PromptKind, content: string) => {
    if (!prompts) return;
    setPrompts({ ...prompts, [kind]: { ...prompts[kind], content } });
  };

  if (!groupId) {
    return (
      <div className={cardClass(isDark)}>
        <div className={`text-sm ${isDark ? "text-slate-300" : "text-gray-700"}`}>{t("guidance.openFromGroup")}</div>
      </div>
    );
  }

  const one = (kind: PromptKind, title: string, hint: string) => {
    const p = prompts?.[kind];
    const source = p?.source || "builtin";
    const badge =
      source === "home"
        ? isDark
          ? "bg-emerald-500/15 text-emerald-300 border border-emerald-500/30"
          : "bg-emerald-50 text-emerald-700 border border-emerald-200"
        : isDark
          ? "bg-slate-800 text-slate-300 border border-slate-700"
          : "bg-gray-100 text-gray-700 border border-gray-200";

    return (
      <div className={cardClass(isDark)}>
        <div className="flex items-start justify-between gap-3">
          <div className="min-w-0">
            <div className={`text-sm font-semibold ${isDark ? "text-slate-100" : "text-gray-900"}`}>{title}</div>
            <div className={`text-[11px] ${isDark ? "text-slate-500" : "text-gray-500"}`}>{hint}</div>
          </div>
          <div className="flex items-center gap-2 shrink-0">
            <button
              type="button"
              className={`px-2 py-1 rounded-md text-[11px] transition-colors ${
                isDark ? "bg-slate-900 hover:bg-slate-800 text-slate-300 border border-slate-800" : "bg-white hover:bg-gray-50 text-gray-700 border border-gray-200"
              }`}
              onClick={() => setExpandedKind(kind)}
              disabled={busy}
              title={t("guidance.expandTitle")}
            >
              {t("guidance.expand")}
            </button>
            <div className={`px-2 py-1 rounded-md text-[11px] ${badge}`}>{source === "home" ? t("guidance.overrideBadge") : t("guidance.builtinBadge")}</div>
          </div>
        </div>

        {p?.path && (
          <div className={preClass(isDark)}>
            <span className="font-mono">{p.path}</span>
          </div>
        )}

        <div className="mt-3">
          <label className={labelClass(isDark)}>{t("guidance.markdown")}</label>
          <textarea
            className={`${inputClass(isDark)} font-mono text-[12px]`}
            style={{ minHeight: 220 }}
            value={p?.content || ""}
            onChange={(e) => setContent(kind, e.target.value)}
            spellCheck={false}
          />
        </div>

        <div className="mt-3 flex items-center gap-2">
          <button className={primaryButtonClass(busy)} onClick={() => save(kind)} disabled={busy}>
            {t("common:save")}
          </button>
          <button
            className={`px-4 py-2 text-sm rounded-lg min-h-[44px] transition-colors font-medium disabled:opacity-50 ${
              isDark ? "bg-slate-800 hover:bg-slate-700 text-slate-200" : "bg-gray-100 hover:bg-gray-200 text-gray-800"
            }`}
            onClick={() => reset(kind)}
            disabled={busy || source !== "home"}
            title={source === "home" ? t("guidance.resetHint") : t("guidance.noOverride")}
          >
            {t("common:reset")}
          </button>
          <button
            className={`ml-auto px-3 py-2 text-sm rounded-lg min-h-[44px] transition-colors disabled:opacity-50 ${
              isDark ? "bg-slate-900 hover:bg-slate-800 text-slate-300 border border-slate-800" : "bg-white hover:bg-gray-50 text-gray-700 border border-gray-200"
            }`}
            onClick={load}
            disabled={busy}
            title={t("guidance.discardChanges")}
          >
            {t("guidance.discardChanges")}
          </button>
        </div>
      </div>
    );
  };

  const expanded = expandedKind ? prompts?.[expandedKind] : null;

  return (
    <div className="space-y-4">
      {err && <div className={`text-sm ${isDark ? "text-rose-300" : "text-red-600"}`}>{err}</div>}
      <div className={`text-[11px] ${isDark ? "text-slate-500" : "text-gray-500"}`}>
        <Trans i18nKey="guidance.overridesHint" ns="settings" components={[<span className="font-mono" />]} />
      </div>
      {one("preamble", t("guidance.preambleTitle"), t("guidance.preambleHint"))}
      {one("help", t("guidance.helpTitle"), t("guidance.helpHint"))}

      {expandedKind && expanded ? (
        <div
          className="fixed inset-0 z-[1000]"
          role="dialog"
          aria-modal="true"
          onPointerDown={(e) => {
            if (e.target === e.currentTarget) setExpandedKind(null);
          }}
        >
          <div className="absolute inset-0 bg-black/50" />
          <div
            className={`absolute inset-0 sm:inset-6 md:inset-10 rounded-none sm:rounded-2xl border ${
              isDark ? "border-slate-800 bg-slate-950" : "border-gray-200 bg-white"
            } shadow-2xl flex flex-col overflow-hidden`}
          >
            <div className={`px-4 py-3 border-b ${isDark ? "border-slate-800" : "border-gray-200"} flex items-start gap-3`}>
              <div className="min-w-0">
                <div className={`text-sm font-semibold ${isDark ? "text-slate-100" : "text-gray-900"}`}>
                  {t("guidance.editKind", { kind: expandedKind })}
                </div>
                {expanded.path ? (
                  <div className={`mt-1 text-[11px] ${isDark ? "text-slate-400" : "text-gray-600"} break-all font-mono`}>
                    {expanded.path}
                  </div>
                ) : null}
              </div>

              <div className="ml-auto flex items-center gap-2">
                <button
                  className={primaryButtonClass(busy)}
                  onClick={() => save(expandedKind)}
                  disabled={busy}
                  title={t("guidance.saveOverrideTitle")}
                >
                  {t("common:save")}
                </button>
                <button
                  type="button"
                  className={`px-4 py-2 text-sm rounded-lg min-h-[44px] transition-colors font-medium disabled:opacity-50 ${
                    isDark ? "bg-slate-800 hover:bg-slate-700 text-slate-200" : "bg-gray-100 hover:bg-gray-200 text-gray-800"
                  }`}
                  onClick={() => reset(expandedKind)}
                  disabled={busy || expanded.source !== "home"}
                  title={expanded.source === "home" ? t("guidance.resetHint") : t("guidance.noOverride")}
                >
                  {t("common:reset")}
                </button>
                <button
                  type="button"
                  className={`px-3 py-2 text-sm rounded-lg min-h-[44px] transition-colors ${
                    isDark ? "bg-slate-900 hover:bg-slate-800 text-slate-300 border border-slate-800" : "bg-white hover:bg-gray-50 text-gray-700 border border-gray-200"
                  }`}
                  onClick={() => setExpandedKind(null)}
                >
                  {t("common:close")}
                </button>
              </div>
            </div>

            <div className="p-4 flex-1 overflow-hidden">
              <textarea
                className={`${inputClass(isDark)} font-mono text-[12px] w-full h-full resize-none`}
                value={expanded.content || ""}
                onChange={(e) => setContent(expandedKind, e.target.value)}
                spellCheck={false}
              />
            </div>
          </div>
        </div>
      ) : null}
    </div>
  );
}
