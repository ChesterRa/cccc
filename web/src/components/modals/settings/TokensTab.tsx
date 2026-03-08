// TokensTab manages User Token CRUD in the Settings modal (global scope).
import { useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import type { GroupMeta } from "../../../types";
import * as api from "../../../services/api";
import { cardClass, inputClass, labelClass, primaryButtonClass } from "./types";

interface TokensTabProps {
  isDark: boolean;
  isActive?: boolean;
  onNavigateToRemote?: () => void;
}

export function TokensTab({ isDark, isActive = true, onNavigateToRemote }: TokensTabProps) {
  const { t } = useTranslation("settings");

  // Token list
  const [tokens, setTokens] = useState<api.UserTokenEntry[]>([]);
  const [loading, setLoading] = useState(false);
  const [err, setErr] = useState("");
  const [tokenAccessDenied, setTokenAccessDenied] = useState(false);

  // Group list for multi-select
  const [groups, setGroups] = useState<GroupMeta[]>([]);

  // Create form
  const [userId, setUserId] = useState("");
  const [customToken, setCustomToken] = useState("");
  const [isAdmin, setIsAdmin] = useState(false);
  const [selectedGroups, setSelectedGroups] = useState<Set<string>>(new Set());

  // 是否存在 admin token（没有时强制首个 token 为 admin）
  const hasAdminToken = tokens.some((t) => t.is_admin);
  const [creating, setCreating] = useState(false);
  const [createErr, setCreateErr] = useState("");

  // Newly created token (shown once)
  const [newToken, setNewToken] = useState<string | null>(null);
  const [copied, setCopied] = useState(false);

  const loadTokens = async () => {
    setLoading(true);
    setErr("");
    try {
      const resp = await api.fetchTokens();
      if (resp.ok && resp.result?.tokens) {
        setTokenAccessDenied(false);
        setTokens(resp.result.tokens);
      } else if (resp.error?.code === "permission_denied") {
        // 首次设置场景：不给裸错误，改为显示引导卡片。
        setTokenAccessDenied(true);
        setTokens([]);
      } else {
        setTokenAccessDenied(false);
        setErr(resp.error?.message || t("tokens.loadFailed"));
      }
    } catch {
      setTokenAccessDenied(false);
      setErr(t("tokens.loadFailed"));
    } finally {
      setLoading(false);
    }
  };

  const loadGroups = async () => {
    try {
      const resp = await api.fetchGroups();
      if (resp.ok && resp.result?.groups) {
        setGroups(resp.result.groups);
      }
    } catch {
      // Non-critical — checkbox list will be empty.
    }
  };

  useEffect(() => {
    if (!isActive) return;
    void loadTokens();
    void loadGroups();
    // eslint-disable-next-line react-hooks/exhaustive-deps -- Load when tab becomes active.
  }, [isActive]);

  const handleCreate = async () => {
    if (tokenAccessDenied) return;
    const uid = userId.trim();
    if (!uid) {
      setCreateErr(t("tokens.userIdRequired"));
      return;
    }
    setCreating(true);
    setCreateErr("");
    try {
      const effectiveAdmin = isAdmin || !hasAdminToken;
      const ct = customToken.trim() || undefined;
      const resp = await api.createToken(uid, effectiveAdmin, [...selectedGroups], ct);
      if (resp.ok && resp.result?.token) {
        const entry = resp.result.token;
        setNewToken(entry.token || null);
        setUserId("");
        setCustomToken("");
        setIsAdmin(false);
        setSelectedGroups(new Set());
        await loadTokens();
      } else {
        setCreateErr(resp.error?.message || t("tokens.createFailed"));
      }
    } catch {
      setCreateErr(t("tokens.createFailed"));
    } finally {
      setCreating(false);
    }
  };

  const handleDelete = async (tokenId: string) => {
    if (!tokenId || !window.confirm(t("tokens.deleteConfirm"))) return;
    try {
      const resp = await api.deleteToken(tokenId);
      if (resp.ok) {
        // If no tokens remain, system reverts to open access — clear stale auth and reload.
        if (resp.result && !(resp.result as Record<string, unknown>).tokens_remain) {
          document.cookie = "cccc_web_token=; path=/; max-age=0";
          window.location.reload();
          return;
        }
        await loadTokens();
      } else {
        setErr(resp.error?.message || t("tokens.deleteFailed"));
      }
    } catch {
      setErr(t("tokens.deleteFailed"));
    }
  };

  const handleCopyToken = async () => {
    if (!newToken) return;
    try {
      await navigator.clipboard.writeText(newToken);
      setCopied(true);
      window.setTimeout(() => setCopied(false), 1500);
    } catch {
      // fallback
      const el = document.createElement("textarea");
      el.value = newToken;
      el.style.position = "fixed";
      el.style.left = "-9999px";
      document.body.appendChild(el);
      el.focus();
      el.select();
      document.execCommand("copy");
      document.body.removeChild(el);
      setCopied(true);
      window.setTimeout(() => setCopied(false), 1500);
    }
  };

  const closeNewToken = () => {
    setNewToken(null);
    setCopied(false);
  };

  // Editing token groups
  const [editingTokenId, setEditingTokenId] = useState<string | null>(null);
  const [editGroups, setEditGroups] = useState<Set<string>>(new Set());
  const [editSaving, setEditSaving] = useState(false);

  const startEdit = (tok: api.UserTokenEntry) => {
    setEditingTokenId(tok.token_id || null);
    setEditGroups(new Set(tok.allowed_groups || []));
  };

  const cancelEdit = () => {
    setEditingTokenId(null);
    setEditGroups(new Set());
  };

  const saveEdit = async () => {
    if (!editingTokenId) return;
    setEditSaving(true);
    try {
      const resp = await api.updateToken(editingTokenId, { allowed_groups: [...editGroups] });
      if (resp.ok) {
        setEditingTokenId(null);
        await loadTokens();
      } else {
        setErr(resp.error?.message || t("tokens.updateFailed"));
      }
    } catch {
      setErr(t("tokens.updateFailed"));
    } finally {
      setEditSaving(false);
    }
  };

  // Reveal + copy full token
  const [copiedTokenId, setCopiedTokenId] = useState<string | null>(null);

  const handleRevealAndCopy = async (tokenId: string) => {
    try {
      const resp = await api.revealToken(tokenId);
      if (resp.ok && resp.result?.token) {
        await navigator.clipboard.writeText(resp.result.token);
        setCopiedTokenId(tokenId);
        window.setTimeout(() => setCopiedTokenId(null), 1500);
      } else {
        setErr(resp.error?.message || t("tokens.revealFailed"));
      }
    } catch {
      setErr(t("tokens.revealFailed"));
    }
  };

  return (
    <div className="space-y-4">
      <div>
        <h3 className={`text-sm font-medium ${isDark ? "text-slate-300" : "text-gray-700"}`}>
          {t("tokens.title")}
        </h3>
        <p className={`text-xs mt-1 ${isDark ? "text-slate-500" : "text-gray-500"}`}>
          {t("tokens.description")}
        </p>
      </div>

      {/* New token banner (shown once after creation) */}
      {newToken && (
        <div
          className={`rounded-lg border px-4 py-3 ${
            isDark
              ? "border-emerald-500/30 bg-emerald-500/10"
              : "border-emerald-200 bg-emerald-50"
          }`}
        >
          <div className={`text-sm font-semibold ${isDark ? "text-emerald-200" : "text-emerald-800"}`}>
            {t("tokens.newTokenTitle")}
          </div>
          <div
            className={`mt-1 text-[11px] ${isDark ? "text-amber-200" : "text-amber-700"}`}
          >
            {t("tokens.newTokenWarning")}
          </div>
          <div className="mt-2 flex items-center gap-2">
            <code
              className={`flex-1 rounded px-2 py-1.5 text-xs break-all select-all ${
                isDark ? "bg-slate-900 text-slate-200" : "bg-white text-gray-800 border border-gray-200"
              }`}
            >
              {newToken}
            </code>
            <button
              onClick={() => void handleCopyToken()}
              className={`px-3 py-2 rounded-lg text-xs min-h-[40px] transition-colors ${
                isDark
                  ? "bg-slate-800 hover:bg-slate-700 text-slate-200"
                  : "bg-white hover:bg-gray-50 text-gray-800 border border-gray-200"
              }`}
            >
              {copied ? t("tokens.copied") : t("tokens.copyToken")}
            </button>
            <button
              onClick={closeNewToken}
              className={`px-3 py-2 rounded-lg text-xs min-h-[40px] transition-colors ${
                isDark
                  ? "bg-slate-800 hover:bg-slate-700 text-slate-200"
                  : "bg-white hover:bg-gray-50 text-gray-800 border border-gray-200"
              }`}
            >
              {t("tokens.close")}
            </button>
          </div>
        </div>
      )}

      {/* Create form */}
      {!tokenAccessDenied && (
        <div className={cardClass(isDark)}>
          <div className={`text-sm font-semibold mb-3 ${isDark ? "text-slate-200" : "text-gray-800"}`}>
            {t("tokens.createTitle")}
          </div>

          <div className="space-y-3">
            <div>
              <label className={labelClass(isDark)}>{t("tokens.userId")}</label>
              <input
                value={userId}
                onChange={(e) => setUserId(e.target.value)}
                placeholder={t("tokens.userIdPlaceholder")}
                className={inputClass(isDark)}
              />
            </div>

            <div>
              <label className={labelClass(isDark)}>{t("tokens.customToken")}</label>
              <input
                value={customToken}
                onChange={(e) => setCustomToken(e.target.value)}
                placeholder={t("tokens.customTokenPlaceholder")}
                className={inputClass(isDark)}
              />
              <div className={`mt-1 text-[11px] ${isDark ? "text-slate-500" : "text-gray-500"}`}>
                {t("tokens.customTokenHint")}
              </div>
            </div>

            {hasAdminToken ? (
              <div>
                <label className="inline-flex items-center gap-2 text-xs select-none cursor-pointer">
                  <input
                    type="checkbox"
                    checked={isAdmin}
                    onChange={(e) => setIsAdmin(e.target.checked)}
                    className="w-4 h-4 accent-indigo-500"
                  />
                  <span className={isDark ? "text-slate-300" : "text-gray-700"}>
                    {t("tokens.isAdmin")}
                  </span>
                </label>
                <div className={`mt-1 text-[11px] ${isDark ? "text-slate-500" : "text-gray-500"}`}>
                  {t("tokens.isAdminHint")}
                </div>
              </div>
            ) : (
              <div className={`text-[11px] px-3 py-2 rounded-lg ${isDark ? "bg-indigo-500/10 text-indigo-300" : "bg-indigo-50 text-indigo-700"}`}>
                {t("tokens.adminRequiredFirst")}
              </div>
            )}

            {hasAdminToken && (
              <div>
                <label className={labelClass(isDark)}>{t("tokens.allowedGroups")}</label>
                {groups.length === 0 ? (
                  <div className={`text-xs ${isDark ? "text-slate-500" : "text-gray-500"}`}>
                    {t("tokens.noGroups")}
                  </div>
                ) : (
                  <div
                    className={`mt-1 rounded-lg border max-h-40 overflow-y-auto ${
                      isDark ? "border-slate-700 bg-slate-900/50" : "border-gray-200 bg-gray-50"
                    }`}
                  >
                    {groups.map((g) => (
                      <label
                        key={g.group_id}
                        className={`flex items-center gap-2 px-3 py-1.5 text-xs select-none cursor-pointer transition-colors ${
                          isDark ? "hover:bg-slate-800/60" : "hover:bg-gray-100"
                        }`}
                      >
                        <input
                          type="checkbox"
                          checked={selectedGroups.has(g.group_id)}
                          onChange={(e) => {
                            setSelectedGroups((prev) => {
                              const next = new Set(prev);
                              if (e.target.checked) next.add(g.group_id);
                              else next.delete(g.group_id);
                              return next;
                            });
                          }}
                          className="w-3.5 h-3.5 accent-indigo-500"
                        />
                        <span className={isDark ? "text-slate-300" : "text-gray-700"}>
                          {g.title || g.group_id}
                        </span>
                        {g.title && (
                          <span className={`text-[10px] ${isDark ? "text-slate-500" : "text-gray-400"}`}>
                            {g.group_id}
                          </span>
                        )}
                      </label>
                    ))}
                  </div>
                )}
                <div className={`mt-1 text-[11px] ${isDark ? "text-slate-500" : "text-gray-500"}`}>
                  {t("tokens.allowedGroupsHint")}
                </div>
              </div>
            )}

            {createErr && (
              <div className={`text-xs ${isDark ? "text-rose-300" : "text-rose-600"}`}>
                {createErr}
              </div>
            )}

            <button
              onClick={() => void handleCreate()}
              disabled={creating}
              className={primaryButtonClass(creating)}
            >
              {creating ? t("tokens.creating") : t("tokens.create")}
            </button>
          </div>
        </div>
      )}

      {/* Token list */}
      <div className={cardClass(isDark)}>
        <div className="flex items-center justify-between gap-2 mb-3">
          <div className={`text-sm font-semibold ${isDark ? "text-slate-200" : "text-gray-800"}`}>
            {t("tokens.tokenListTitle")}
          </div>
          <button
            onClick={() => void loadTokens()}
            disabled={loading}
            className={`px-3 py-2 rounded-lg text-xs min-h-[40px] transition-colors ${
              isDark
                ? "bg-slate-800 hover:bg-slate-700 text-slate-200"
                : "bg-white hover:bg-gray-50 text-gray-800 border border-gray-200"
            } disabled:opacity-50`}
          >
            {loading ? t("common:loading") : t("tokens.refresh")}
          </button>
        </div>

        {tokenAccessDenied ? (
          <div
            className={`mb-3 rounded-lg border px-4 py-3 ${
              isDark ? "border-amber-500/30 bg-amber-500/10" : "border-amber-200 bg-amber-50"
            }`}
          >
            <div className={`text-sm font-semibold ${isDark ? "text-amber-200" : "text-amber-800"}`}>
              {t("tokens.accessDeniedTitle")}
            </div>
            <div className={`mt-1 text-xs ${isDark ? "text-slate-300" : "text-gray-700"}`}>
              {t("tokens.accessDeniedBody")}
            </div>
            <div className={`mt-1 text-[11px] ${isDark ? "text-slate-400" : "text-gray-600"}`}>
              {t("tokens.accessDeniedHint")}
            </div>
            <div className="mt-3 flex flex-wrap gap-2">
              <button
                onClick={() => onNavigateToRemote?.()}
                className={`px-3 py-2 rounded-lg text-xs min-h-[40px] transition-colors ${
                  isDark
                    ? "bg-slate-800 hover:bg-slate-700 text-slate-200"
                    : "bg-white hover:bg-gray-50 text-gray-800 border border-gray-200"
                }`}
              >
                {t("tokens.openRemoteAccess")}
              </button>
            </div>
          </div>
        ) : err && (
          <div className={`mb-3 text-xs ${isDark ? "text-rose-300" : "text-rose-600"}`}>
            {err}
          </div>
        )}

        {tokens.length === 0 && !loading && !err && !tokenAccessDenied ? (
          <div className={`text-xs ${isDark ? "text-slate-500" : "text-gray-500"}`}>
            {t("tokens.noTokens")}
          </div>
        ) : !tokenAccessDenied ? (
          <div className="space-y-2">
            {tokens.map((tok, idx) => (
              <div
                key={`${tok.token_preview || ""}-${idx}`}
                className={`rounded-lg border px-3 py-2.5 ${
                  isDark ? "border-slate-800 bg-slate-950/30" : "border-gray-200 bg-white"
                }`}
              >
                <div className="flex items-start justify-between gap-2">
                  <div className="flex-1 min-w-0">
                    <div className="flex items-center gap-2 flex-wrap">
                      <code
                        className={`text-xs px-1.5 py-0.5 rounded ${
                          isDark ? "bg-slate-800 text-slate-300" : "bg-gray-100 text-gray-700"
                        }`}
                      >
                        {tok.token_preview || "****"}
                      </code>
                      <span
                        className={`text-[11px] px-1.5 py-0.5 rounded ${
                          tok.is_admin
                            ? isDark
                              ? "bg-amber-900/30 text-amber-300 border border-amber-700/40"
                              : "bg-amber-50 text-amber-700 border border-amber-200"
                            : isDark
                              ? "bg-slate-800 text-slate-400"
                              : "bg-gray-100 text-gray-600"
                        }`}
                      >
                        {tok.is_admin ? t("tokens.admin") : t("tokens.user")}
                      </span>
                    </div>
                    <div className={`mt-1 text-[11px] ${isDark ? "text-slate-500" : "text-gray-500"}`}>
                      <span className="font-medium">{tok.user_id}</span>
                      {" · "}
                      {t("tokens.groups")}:{" "}
                      {tok.allowed_groups && tok.allowed_groups.length > 0
                        ? tok.allowed_groups.join(", ")
                        : t("tokens.allGroups")}
                      {tok.created_at && (
                        <>
                          {" · "}
                          {t("tokens.createdAt")}: {new Date(tok.created_at).toLocaleString()}
                        </>
                      )}
                    </div>
                  </div>
                  <div className="flex items-center gap-1.5 shrink-0">
                    {!tok.is_admin && editingTokenId !== (tok.token_id || "") && (
                      <button
                        onClick={() => startEdit(tok)}
                        className={`px-2.5 py-1.5 rounded-lg text-[11px] min-h-[32px] transition-colors ${
                          isDark
                            ? "bg-slate-800 hover:bg-slate-700 text-slate-300"
                            : "bg-gray-100 hover:bg-gray-200 text-gray-700"
                        }`}
                      >
                        {t("tokens.edit")}
                      </button>
                    )}
                    <button
                      onClick={() => void handleRevealAndCopy(tok.token_id || "")}
                      className={`px-2.5 py-1.5 rounded-lg text-[11px] min-h-[32px] transition-colors ${
                        isDark
                          ? "bg-slate-800 hover:bg-slate-700 text-slate-300"
                          : "bg-gray-100 hover:bg-gray-200 text-gray-700"
                      }`}
                    >
                      {copiedTokenId === (tok.token_id || "") ? t("tokens.copied") : t("tokens.copyFullToken")}
                    </button>
                    <button
                      onClick={() => void handleDelete(tok.token_id || "")}
                      className={`px-2.5 py-1.5 rounded-lg text-[11px] min-h-[32px] transition-colors ${
                        isDark
                          ? "bg-rose-900/30 hover:bg-rose-800/40 text-rose-300 border border-rose-700/40"
                          : "bg-rose-50 hover:bg-rose-100 text-rose-600 border border-rose-200"
                      }`}
                    >
                      {t("tokens.delete")}
                    </button>
                  </div>
                </div>
                {/* Inline edit: allowed groups */}
                {editingTokenId === (tok.token_id || "") && (
                  <div className={`mt-2 pt-2 border-t ${isDark ? "border-slate-800" : "border-gray-200"}`}>
                    <label className={`text-[11px] font-medium ${isDark ? "text-slate-400" : "text-gray-600"}`}>
                      {t("tokens.allowedGroups")}
                    </label>
                    {groups.length === 0 ? (
                      <div className={`text-xs mt-1 ${isDark ? "text-slate-500" : "text-gray-500"}`}>
                        {t("tokens.noGroups")}
                      </div>
                    ) : (
                      <div
                        className={`mt-1 rounded-lg border max-h-32 overflow-y-auto ${
                          isDark ? "border-slate-700 bg-slate-900/50" : "border-gray-200 bg-gray-50"
                        }`}
                      >
                        {groups.map((g) => (
                          <label
                            key={g.group_id}
                            className={`flex items-center gap-2 px-3 py-1.5 text-xs select-none cursor-pointer transition-colors ${
                              isDark ? "hover:bg-slate-800/60" : "hover:bg-gray-100"
                            }`}
                          >
                            <input
                              type="checkbox"
                              checked={editGroups.has(g.group_id)}
                              onChange={(e) => {
                                setEditGroups((prev) => {
                                  const next = new Set(prev);
                                  if (e.target.checked) next.add(g.group_id);
                                  else next.delete(g.group_id);
                                  return next;
                                });
                              }}
                              className="w-3.5 h-3.5 accent-indigo-500"
                            />
                            <span className={isDark ? "text-slate-300" : "text-gray-700"}>
                              {g.title || g.group_id}
                            </span>
                            {g.title && (
                              <span className={`text-[10px] ${isDark ? "text-slate-500" : "text-gray-400"}`}>
                                {g.group_id}
                              </span>
                            )}
                          </label>
                        ))}
                      </div>
                    )}
                    <div className={`mt-1 text-[11px] ${isDark ? "text-slate-500" : "text-gray-500"}`}>
                      {t("tokens.allowedGroupsHint")}
                    </div>
                    <div className="mt-2 flex gap-2">
                      <button
                        onClick={() => void saveEdit()}
                        disabled={editSaving}
                        className={`px-2.5 py-1.5 rounded-lg text-[11px] min-h-[32px] transition-colors ${
                          isDark
                            ? "bg-indigo-600 hover:bg-indigo-500 text-white"
                            : "bg-indigo-500 hover:bg-indigo-600 text-white"
                        } disabled:opacity-50`}
                      >
                        {editSaving ? t("common:loading") : t("tokens.save")}
                      </button>
                      <button
                        onClick={cancelEdit}
                        className={`px-2.5 py-1.5 rounded-lg text-[11px] min-h-[32px] transition-colors ${
                          isDark
                            ? "bg-slate-800 hover:bg-slate-700 text-slate-300"
                            : "bg-gray-100 hover:bg-gray-200 text-gray-700"
                        }`}
                      >
                        {t("tokens.cancel")}
                      </button>
                    </div>
                  </div>
                )}
              </div>
            ))}
          </div>
        ) : null}
      </div>
    </div>
  );
}
