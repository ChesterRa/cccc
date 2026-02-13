import { RuntimeInfo, SupportedRuntime, SUPPORTED_RUNTIMES, RUNTIME_INFO } from "../../types";
import { useTranslation } from "react-i18next";
import { BASIC_MCP_CONFIG_SNIPPET, COPILOT_MCP_CONFIG_SNIPPET, OPENCODE_MCP_CONFIG_SNIPPET } from "../../utils/mcpConfigSnippets";
import { useEffect, useState } from "react";
import * as api from "../../services/api";
import { useModalA11y } from "../../hooks/useModalA11y";
import { parsePrivateEnvSetText, parsePrivateEnvUnsetText } from "../../utils/privateEnvInput";

export interface EditActorModalProps {
  isOpen: boolean;
  isDark: boolean;
  busy: string;
  groupId: string;
  actorId: string;
  isRunning: boolean;
  runtimes: RuntimeInfo[];
  runtime: SupportedRuntime;
  onChangeRuntime: (runtime: SupportedRuntime) => void;
  command: string;
  onChangeCommand: (command: string) => void;
  title: string;
  onChangeTitle: (title: string) => void;
  onSaveAndRestart: (secrets: { setVars: Record<string, string>; unsetKeys: string[]; clear: boolean }) => Promise<void>;
  onCancel: () => void;
}

/** Runtime-specific placeholder hints for secret environment variables */
const SECRETS_PLACEHOLDER: Record<string, { set: string; unset: string }> = {
  claude: {
    set: 'export ANTHROPIC_API_KEY="...";\nexport ANTHROPIC_BASE_URL="...";',
    unset: "unset ANTHROPIC_API_KEY;\nunset ANTHROPIC_BASE_URL;",
  },
  codex: {
    set: 'export OPENAI_API_KEY="...";\nexport OPENAI_BASE_URL="...";',
    unset: "unset OPENAI_API_KEY;\nunset OPENAI_BASE_URL;",
  },
  gemini: {
    set: 'export GOOGLE_API_KEY="...";',
    unset: "unset GOOGLE_API_KEY;",
  },
};

const DEFAULT_SECRETS_PLACEHOLDER = {
  set: 'export OPENAI_API_KEY="...";\nexport ANTHROPIC_API_KEY="...";\nexport ANTHROPIC_BASE_URL="...";',
  unset: "unset OPENAI_API_KEY;\nunset ANTHROPIC_API_KEY;\nunset ANTHROPIC_BASE_URL;",
};

export function EditActorModal({
  isOpen,
  isDark,
  busy,
  groupId,
  actorId,
  isRunning,
  runtimes,
  runtime,
  onChangeRuntime,
  command,
  onChangeCommand,
  title,
  onChangeTitle,
  onSaveAndRestart,
  onCancel,
}: EditActorModalProps) {
  const { t } = useTranslation('actors');
  const { modalRef } = useModalA11y(isOpen, onCancel);
  const [secretKeys, setSecretKeys] = useState<string[]>([]);
  const [secretsSetText, setSecretsSetText] = useState("");
  const [secretsUnsetText, setSecretsUnsetText] = useState("");
  const [secretsClearAll, setSecretsClearAll] = useState(false);
  const [secretsError, setSecretsError] = useState("");
  const [secretsBusy, setSecretsBusy] = useState(false);

  const secretsPlaceholder = SECRETS_PLACEHOLDER[runtime] ?? DEFAULT_SECRETS_PLACEHOLDER;

  const refreshSecretKeys = async () => {
    if (!groupId || !actorId) return;
    const resp = await api.fetchActorPrivateEnvKeys(groupId, actorId);
    if (!resp.ok) {
      setSecretsError(resp.error?.message || t('failedToLoadSecrets'));
      return;
    }
    setSecretKeys(Array.isArray(resp.result?.keys) ? resp.result.keys : []);
  };

  useEffect(() => {
    setSecretsError("");
    setSecretsSetText("");
    setSecretsUnsetText("");
    setSecretsClearAll(false);
    if (isOpen) void refreshSecretKeys();
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [groupId, actorId, isOpen]);

  if (!isOpen) return null;

  const rtInfo = runtimes.find((r) => r.name === runtime);
  const available = rtInfo?.available ?? false;
  const defaultCommand = rtInfo?.recommended_command || "";
  const requireCommand = runtime === "custom" || !available;

  const saveAndRestart = async () => {
    if (!groupId || !actorId) return;
    if (busy === "actor-update") return;
    setSecretsError("");

    const setParsed = parsePrivateEnvSetText(secretsSetText);
    if (!setParsed.ok) {
      setSecretsError(setParsed.error);
      return;
    }
    const unsetParsed = parsePrivateEnvUnsetText(secretsUnsetText);
    if (!unsetParsed.ok) {
      setSecretsError(unsetParsed.error);
      return;
    }

    setSecretsBusy(true);
    try {
      await onSaveAndRestart({
        setVars: setParsed.setVars,
        unsetKeys: unsetParsed.unsetKeys,
        clear: secretsClearAll,
      });
    } catch (e) {
      setSecretsError(e instanceof Error ? e.message : t('saveFailed'));
      return;
    } finally {
      setSecretsBusy(false);
    }
  };

  return (
    <div
      className={`fixed inset-0 flex items-stretch sm:items-start justify-center p-0 sm:p-6 z-50 animate-fade-in ${isDark ? "bg-black/60" : "bg-black/40"}`}
      onPointerDown={(e) => {
        if (e.target === e.currentTarget) onCancel();
      }}
      role="dialog"
      aria-modal="true"
      aria-labelledby="edit-actor-title"
    >
      <div
        ref={modalRef}
        className={`w-full h-full sm:h-auto sm:max-w-md sm:mt-16 border shadow-2xl animate-scale-in flex flex-col sm:max-h-[calc(100vh-8rem)] rounded-none sm:rounded-2xl ${
          isDark ? "border-slate-700/50 bg-gradient-to-b from-slate-800 to-slate-900" : "border-gray-200 bg-white"
        }`}
      >
        <div className={`px-6 py-4 border-b safe-area-inset-top ${isDark ? "border-slate-700/50" : "border-gray-200"}`}>
          <div id="edit-actor-title" className={`text-lg font-semibold ${isDark ? "text-white" : "text-gray-900"}`}>
            {t('editAgent', { actorId })}
          </div>
          <div className={`text-sm mt-1 ${isDark ? "text-slate-400" : "text-gray-500"}`}>{t('changeSettings')}</div>
        </div>
        <div className="p-6 space-y-5 overflow-y-auto flex-1 min-h-0">
          <div>
            <label className={`block text-xs font-medium mb-2 ${isDark ? "text-slate-400" : "text-gray-500"}`}>{t('displayName')}</label>
            <input
              className={`w-full rounded-xl border px-4 py-2.5 text-sm min-h-[44px] transition-colors ${
                isDark ? "bg-slate-900/80 border-slate-600/50 text-white focus:border-blue-500" : "bg-white border-gray-300 text-gray-900 focus:border-blue-500"
              }`}
              value={title}
              onChange={(e) => onChangeTitle(e.target.value)}
              placeholder={actorId}
            />
            <div className={`text-[10px] mt-1.5 ${isDark ? "text-slate-500" : "text-gray-500"}`}>
              {t('leaveEmptyForId')}
            </div>
          </div>

          <div>
            <label className={`block text-xs font-medium mb-2 ${isDark ? "text-slate-400" : "text-gray-500"}`}>{t('runtime')}</label>
            <select
              className={`w-full rounded-xl border px-4 py-2.5 text-sm min-h-[44px] transition-colors ${
                isDark ? "bg-slate-900/80 border-slate-600/50 text-white focus:border-blue-500" : "bg-white border-gray-300 text-gray-900 focus:border-blue-500"
              }`}
              value={runtime}
              onChange={(e) => {
                const next = e.target.value as SupportedRuntime;
                onChangeRuntime(next);
                const nextInfo = runtimes.find((r) => r.name === next);
                const nextDefault = String(nextInfo?.recommended_command || "").trim();
                onChangeCommand(nextDefault);
              }}
            >
              {SUPPORTED_RUNTIMES.map((rt) => {
                const info = RUNTIME_INFO[rt];
                const rtInfo = runtimes.find((r) => r.name === rt);
                const available = rtInfo?.available ?? false;
                const selectable = available || rt === "custom";
                return (
                  <option key={rt} value={rt} disabled={!selectable}>
                    {info?.label || rt}
                    {!available && rt !== "custom" ? ` ${t('notInstalled')}` : ""}
                  </option>
                );
              })}
            </select>

            {(runtime === "cursor" || runtime === "kilocode" || runtime === "opencode" || runtime === "copilot" || runtime === "custom") && (
              <div
                className={`mt-2 rounded-xl border px-3 py-2 text-[11px] ${
                  isDark ? "border-amber-500/30 bg-amber-500/10 text-amber-200" : "border-amber-200 bg-amber-50 text-amber-800"
                }`}
              >
                <div className="font-medium">{t('manualMcpRequired')}</div>
                {runtime === "custom" ? (
                  <>
                    <div className="mt-1">
                      {t('configureMcpStdio')}{" "}
                      <code className={`px-1 rounded ${isDark ? "bg-amber-900/30" : "bg-amber-100"}`}>cccc</code>{" "}
                      {t('thatRuns')} <code className={`px-1 rounded ${isDark ? "bg-amber-900/30" : "bg-amber-100"}`}>cccc mcp</code>.
                    </div>
                  </>
                ) : runtime === "cursor" ? (
                  <>
                    <div className="mt-1">
                      {t('createEditFile')}{" "}
                      <code className={`px-1 rounded ${isDark ? "bg-amber-900/30" : "bg-amber-100"}`}>~/.cursor/mcp.json</code> (or{" "}
                      <code className={`px-1 rounded ${isDark ? "bg-amber-900/30" : "bg-amber-100"}`}>.cursor/mcp.json</code> {t('orInProject')})
                    </div>
                    <div className="mt-1">{t('addMcpConfig')}</div>
                  </>
                ) : runtime === "kilocode" ? (
                  <>
                    <div className="mt-1">
                      {t('createEditFile')}{" "}
                      <code className={`px-1 rounded ${isDark ? "bg-amber-900/30" : "bg-amber-100"}`}>.kilocode/mcp.json</code> {t('inProjectRoot')}
                    </div>
                    <div className="mt-1">{t('addMcpConfig')}</div>
                  </>
                ) : runtime === "opencode" ? (
                  <>
                    <div className="mt-1">
                      {t('createEditFile')}{" "}
                      <code className={`px-1 rounded ${isDark ? "bg-amber-900/30" : "bg-amber-100"}`}>~/.config/opencode/opencode.json</code>
                    </div>
                    <div className="mt-1">{t('addMcpConfig')}</div>
                  </>
                ) : (
                  <>
                    <div className="mt-1">
                      {t('createEditFile')}{" "}
                      <code className={`px-1 rounded ${isDark ? "bg-amber-900/30" : "bg-amber-100"}`}>~/.copilot/mcp-config.json</code>
                    </div>
                    <div className="mt-1">
                      {t('addMcpConfigOrFlag')}{" "}
                      <code className={`px-1 rounded ${isDark ? "bg-amber-900/30" : "bg-amber-100"}`}>--additional-mcp-config</code>):
                    </div>
                  </>
                )}
                {runtime !== "custom" ? (
                  <pre
                    className={`mt-1.5 p-2 rounded overflow-x-auto whitespace-pre ${
                      isDark ? "bg-amber-900/20 text-amber-100" : "bg-amber-50 text-amber-900"
                    }`}
                  >
                    <code>
                      {runtime === "opencode" ? OPENCODE_MCP_CONFIG_SNIPPET : runtime === "copilot" ? COPILOT_MCP_CONFIG_SNIPPET : BASIC_MCP_CONFIG_SNIPPET}
                    </code>
                  </pre>
                ) : null}
                <div className={`mt-1 text-[10px] ${isDark ? "text-amber-200/80" : "text-amber-800/80"}`}>
                  {t('restartAfterConfig')}
                </div>
              </div>
            )}
          </div>

          <div>
            <label className={`block text-xs font-medium mb-2 ${isDark ? "text-slate-400" : "text-gray-500"}`}>{t('command')}</label>
            <input
              className={`w-full rounded-xl border px-4 py-2.5 text-sm font-mono min-h-[44px] transition-colors ${
                isDark ? "bg-slate-900/80 border-slate-600/50 text-white focus:border-blue-500" : "bg-white border-gray-300 text-gray-900 focus:border-blue-500"
              }`}
              value={command}
              onChange={(e) => onChangeCommand(e.target.value)}
              placeholder={defaultCommand || t('enterCommand')}
            />
            {isRunning ? (
              <div className={`text-[10px] mt-1.5 ${isDark ? "text-slate-500" : "text-gray-500"}`}>
                {t('runtimeChangesNote')}
              </div>
            ) : null}
            {defaultCommand.trim() ? (
              <div className={`text-[10px] mt-1.5 ${isDark ? "text-slate-500" : "text-gray-500"}`}>
                {t('default')}{" "}
                <code className={`px-1 rounded ${isDark ? "bg-slate-800" : "bg-gray-100"}`}>{defaultCommand}</code>
              </div>
            ) : null}
          </div>

          <div>
            <div className="flex items-center justify-between gap-3">
              <label className={`block text-xs font-medium ${isDark ? "text-slate-400" : "text-gray-500"}`}>{t('secretsWriteOnly')}</label>
              <button
                className={`text-xs px-2 py-1 rounded-lg border transition-colors ${
                  isDark ? "border-slate-600/50 text-slate-300 hover:bg-slate-800" : "border-gray-200 text-gray-700 hover:bg-gray-50"
                }`}
                onClick={() => void refreshSecretKeys()}
                disabled={secretsBusy}
                title={t('refreshConfiguredKeys')}
              >
                {t('refresh')}
              </button>
            </div>
            <div className={`text-[10px] mt-1.5 ${isDark ? "text-slate-500" : "text-gray-500"}`}>
              {t('secretsStoredLocallyEdit').replace(/<1>|<\/1>/g, '')}{" "}
              {secretKeys.length ? (
                <>
                  {t('configuredKeys')}{" "}
                  <span className={isDark ? "text-slate-300" : "text-gray-700"}>{secretKeys.join(", ")}</span>
                </>
              ) : (
                <>{t('noKeysConfigured')}</>
              )}
            </div>
            <div className={`text-[10px] mt-1 ${isDark ? "text-slate-500" : "text-gray-500"}`}>
              {t('secretsAppliedNote').replace(/<1>|<\/1>/g, '')}
            </div>

            <label className={`block text-[11px] font-medium mt-3 mb-1.5 ${isDark ? "text-slate-500" : "text-gray-600"}`}>
              {t('setUpdate')}
            </label>
            <textarea
              className={`w-full rounded-xl border px-3 py-2 text-sm font-mono min-h-[96px] transition-colors ${
                isDark ? "bg-slate-900/80 border-slate-600/50 text-white focus:border-blue-500" : "bg-white border-gray-300 text-gray-900 focus:border-blue-500"
              }`}
              value={secretsSetText}
              onChange={(e) => setSecretsSetText(e.target.value)}
              placeholder={secretsPlaceholder.set}
            />

            <label className={`block text-[11px] font-medium mt-3 mb-1.5 ${isDark ? "text-slate-500" : "text-gray-600"}`}>
              {t('unset')}
            </label>
            <textarea
              className={`w-full rounded-xl border px-3 py-2 text-sm font-mono min-h-[72px] transition-colors ${
                isDark ? "bg-slate-900/80 border-slate-600/50 text-white focus:border-blue-500" : "bg-white border-gray-300 text-gray-900 focus:border-blue-500"
              }`}
              value={secretsUnsetText}
              onChange={(e) => setSecretsUnsetText(e.target.value)}
              placeholder={secretsPlaceholder.unset}
            />

            <label className={`flex items-center gap-2 text-[11px] font-medium mt-3 ${isDark ? "text-slate-400" : "text-gray-600"}`}>
              <input
                type="checkbox"
                checked={secretsClearAll}
                onChange={(e) => setSecretsClearAll(e.target.checked)}
                disabled={secretsBusy || busy === "actor-update"}
              />
              {t('clearAllKeys')}
            </label>

            {secretsError ? (
              <div
                className={`mt-2 rounded-xl border px-3 py-2 text-xs ${
                  isDark ? "border-rose-500/30 bg-rose-500/10 text-rose-300" : "border-rose-300 bg-rose-50 text-rose-700"
                }`}
                role="alert"
              >
                {secretsError}
              </div>
            ) : null}
          </div>
          <div className="flex gap-3 pt-2">
            <button
              className="flex-1 rounded-xl bg-blue-600 hover:bg-blue-500 text-white px-4 py-2.5 text-sm font-semibold shadow-lg disabled:opacity-50 transition-all min-h-[44px]"
              onClick={() => void saveAndRestart()}
              disabled={busy === "actor-update" || secretsBusy || (requireCommand && !command.trim())}
            >
              {t('saveAndRestart')}
            </button>
            <button
              className={`px-4 py-2.5 rounded-xl text-sm font-medium transition-colors min-h-[44px] ${
                isDark ? "bg-slate-700 hover:bg-slate-600 text-slate-200" : "bg-gray-100 hover:bg-gray-200 text-gray-700"
              }`}
              onClick={onCancel}
            >
              {t('common:cancel')}
            </button>
          </div>
        </div>
      </div>
    </div>
  );
}
