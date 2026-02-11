import { useEffect, useState } from "react";
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
        setErr(resp.error?.message || "Failed to load guidance");
        setPrompts(null);
        return;
      }
      const p = resp.result?.preamble;
      const h = resp.result?.help;
      if (!p || !h) {
        setErr("Invalid guidance response");
        setPrompts(null);
        return;
      }
      setPrompts({ preamble: p, help: h });
    } catch {
      setErr("Failed to load guidance");
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
        setErr(resp.error?.message || `Failed to save ${kind}`);
        return;
      }
      await load();
    } catch {
      setErr(`Failed to save ${kind}`);
    } finally {
      setBusy(false);
    }
  };

  const reset = async (kind: PromptKind) => {
    if (!groupId) return;
    const filename = prompts?.[kind]?.filename || kind;
    const ok = window.confirm(`Reset ${kind}? This will delete ${filename} override under CCCC_HOME. This cannot be undone.`);
    if (!ok) return;

    setBusy(true);
    setErr("");
    try {
      const resp = await apiJson<PromptInfo>(
        `/api/v1/groups/${encodeURIComponent(groupId)}/prompts/${kind}?confirm=${encodeURIComponent(kind)}`,
        { method: "DELETE" }
      );
      if (!resp.ok) {
        setErr(resp.error?.message || `Failed to reset ${kind}`);
        return;
      }
      await load();
    } catch {
      setErr(`Failed to reset ${kind}`);
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
        <div className={`text-sm ${isDark ? "text-slate-300" : "text-gray-700"}`}>Open this tab from a group.</div>
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
              title="Open full-screen editor"
            >
              Expand
            </button>
            <div className={`px-2 py-1 rounded-md text-[11px] ${badge}`}>{source === "home" ? "Override" : "Built-in"}</div>
          </div>
        </div>

        {p?.path && (
          <div className={preClass(isDark)}>
            <span className="font-mono">{p.path}</span>
          </div>
        )}

        <div className="mt-3">
          <label className={labelClass(isDark)}>Markdown</label>
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
            Save
          </button>
          <button
            className={`px-4 py-2 text-sm rounded-lg min-h-[44px] transition-colors font-medium disabled:opacity-50 ${
              isDark ? "bg-slate-800 hover:bg-slate-700 text-slate-200" : "bg-gray-100 hover:bg-gray-200 text-gray-800"
            }`}
            onClick={() => reset(kind)}
            disabled={busy || source !== "home"}
            title={source === "home" ? "Delete override file and fall back to built-in" : "No override file to delete"}
          >
            Reset
          </button>
          <button
            className={`ml-auto px-3 py-2 text-sm rounded-lg min-h-[44px] transition-colors disabled:opacity-50 ${
              isDark ? "bg-slate-900 hover:bg-slate-800 text-slate-300 border border-slate-800" : "bg-white hover:bg-gray-50 text-gray-700 border border-gray-200"
            }`}
            onClick={load}
            disabled={busy}
            title="Discard local edits and reload from server"
          >
            Discard Changes
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
        Guidance overrides are stored under <span className="font-mono">CCCC_HOME</span> (per-group).
      </div>
      {one("preamble", "Bootstrap (Preamble)", "Injected automatically before the first delivery after start/restart.")}
      {one("help", "Help (Reference)", "Returned by cccc_help (on-demand).")}

      {expandedKind && expanded ? (
        <div
          className="fixed inset-0 z-[1000]"
          role="dialog"
          aria-modal="true"
          onMouseDown={(e) => {
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
                  Edit {expandedKind}
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
                  title="Save override under CCCC_HOME"
                >
                  Save
                </button>
                <button
                  type="button"
                  className={`px-4 py-2 text-sm rounded-lg min-h-[44px] transition-colors font-medium disabled:opacity-50 ${
                    isDark ? "bg-slate-800 hover:bg-slate-700 text-slate-200" : "bg-gray-100 hover:bg-gray-200 text-gray-800"
                  }`}
                  onClick={() => reset(expandedKind)}
                  disabled={busy || expanded.source !== "home"}
                  title={expanded.source === "home" ? "Delete override and fall back to built-in" : "No override file to delete"}
                >
                  Reset
                </button>
                <button
                  type="button"
                  className={`px-3 py-2 text-sm rounded-lg min-h-[44px] transition-colors ${
                    isDark ? "bg-slate-900 hover:bg-slate-800 text-slate-300 border border-slate-800" : "bg-white hover:bg-gray-50 text-gray-700 border border-gray-200"
                  }`}
                  onClick={() => setExpandedKind(null)}
                >
                  Close
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
