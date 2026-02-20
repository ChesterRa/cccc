import { useEffect, useMemo, useState } from "react";
import { useTranslation } from "react-i18next";
import { ActorProfile, ActorProfileUsage, RUNTIME_INFO, SUPPORTED_RUNTIMES } from "../../../types";
import * as api from "../../../services/api";
import { parsePrivateEnvSetText, parsePrivateEnvUnsetText } from "../../../utils/privateEnvInput";
import { useGroupStore } from "../../../stores";
import { cardClass, inputClass, labelClass, primaryButtonClass } from "./types";

interface ActorProfilesTabProps {
  isDark: boolean;
  isActive: boolean;
}

type EditorState = {
  id: string;
  revision: number;
  name: string;
  runtime: string;
  runner: "pty" | "headless";
  command: string;
  useDefaultCommand: boolean;
  submit: "enter" | "newline" | "none";
};

function formatCommand(cmd: string[] | undefined): string {
  const parts = Array.isArray(cmd) ? cmd.filter((item) => typeof item === "string" && item.trim()) : [];
  return parts.join(" ");
}

const RUNTIME_DEFAULT_COMMANDS: Record<string, string> = {
  amp: "amp",
  auggie: "auggie",
  claude: "claude --dangerously-skip-permissions",
  codex: "codex -c shell_environment_policy.inherit=all --dangerously-bypass-approvals-and-sandbox --search",
  cursor: "cursor-agent",
  droid: "droid --auto high",
  gemini: "gemini --yolo",
  kilocode: "kilocode",
  neovate: "neovate",
  opencode: "opencode",
  copilot: "copilot --allow-all-tools --allow-all-paths",
  custom: "",
};

function defaultCommandForRuntime(runtime: string): string {
  const key = String(runtime || "").trim();
  return String(RUNTIME_DEFAULT_COMMANDS[key] || key || "").trim();
}

function supportsRuntimeDefaultCommand(runtime: string, runner: "pty" | "headless"): boolean {
  return runner !== "headless" && String(runtime || "").trim() !== "custom";
}

function buildEditor(profile?: ActorProfile | null): EditorState {
  const runtime = String(profile?.runtime || "codex");
  const runner = (String(profile?.runner || "pty") === "headless" ? "headless" : "pty");
  const command = formatCommand(profile?.command);
  const defaultCommand = defaultCommandForRuntime(runtime);
  const useDefaultCommand =
    supportsRuntimeDefaultCommand(runtime, runner) &&
    (!command.trim() || command.trim() === defaultCommand);
  return {
    id: String(profile?.id || ""),
    revision: Number(profile?.revision || 0),
    name: String(profile?.name || ""),
    runtime,
    runner,
    command,
    useDefaultCommand,
    submit: (String(profile?.submit || "enter") as "enter" | "newline" | "none"),
  };
}

export function ActorProfilesTab({ isDark, isActive }: ActorProfilesTabProps) {
  const { t } = useTranslation("settings");
  const refreshActors = useGroupStore((s) => s.refreshActors);
  const [profiles, setProfiles] = useState<ActorProfile[]>([]);
  const [busy, setBusy] = useState(false);
  const [err, setErr] = useState("");
  const [query, setQuery] = useState("");

  const [editorOpen, setEditorOpen] = useState(false);
  const [editorBusy, setEditorBusy] = useState(false);
  const [editorErr, setEditorErr] = useState("");
  const [editor, setEditor] = useState<EditorState>(buildEditor());
  const [secretKeys, setSecretKeys] = useState<string[]>([]);
  const [secretMasks, setSecretMasks] = useState<Record<string, string>>({});
  const [secretSetText, setSecretSetText] = useState("");
  const [secretUnsetText, setSecretUnsetText] = useState("");
  const [secretClear, setSecretClear] = useState(false);
  const [duplicateSourceProfileId, setDuplicateSourceProfileId] = useState("");

  const filtered = useMemo(() => {
    const q = query.trim().toLowerCase();
    if (!q) return profiles;
    return profiles.filter((item) => {
      const name = String(item.name || "").toLowerCase();
      const id = String(item.id || "").toLowerCase();
      const rt = String(item.runtime || "").toLowerCase();
      return name.includes(q) || id.includes(q) || rt.includes(q);
    });
  }, [profiles, query]);

  const duplicateSourceLabel = useMemo(() => {
    const sourceId = duplicateSourceProfileId.trim();
    if (!sourceId) return "";
    const profile = profiles.find((item) => String(item.id || "").trim() === sourceId);
    if (!profile) return sourceId;
    return String(profile.name || profile.id || sourceId).trim() || sourceId;
  }, [duplicateSourceProfileId, profiles]);

  const editorSupportsDefaultCommand = useMemo(
    () => supportsRuntimeDefaultCommand(editor.runtime, editor.runner),
    [editor.runtime, editor.runner]
  );
  const editorDefaultCommand = useMemo(
    () => defaultCommandForRuntime(editor.runtime),
    [editor.runtime]
  );

  const loadProfiles = async () => {
    setBusy(true);
    setErr("");
    try {
      const resp = await api.listActorProfiles();
      if (resp.ok) {
        setProfiles(Array.isArray(resp.result?.profiles) ? resp.result.profiles : []);
      } else {
        setErr(resp.error?.message || t("actorProfiles.loadFailed"));
      }
    } catch {
      setErr(t("actorProfiles.loadFailed"));
    } finally {
      setBusy(false);
    }
  };

  const loadProfileSecrets = async (profileId: string) => {
    if (!profileId) {
      setSecretKeys([]);
      setSecretMasks({});
      return;
    }
    const resp = await api.fetchActorProfilePrivateEnvKeys(profileId);
    if (!resp.ok) {
      setEditorErr(resp.error?.message || t("actorProfiles.loadSecretsFailed"));
      setSecretKeys([]);
      setSecretMasks({});
      return;
    }
    const keys = Array.isArray(resp.result?.keys) ? resp.result.keys : [];
    const masked = (resp.result?.masked_values && typeof resp.result.masked_values === "object")
      ? resp.result.masked_values
      : {};
    setSecretKeys(keys);
    setSecretMasks(masked);
  };

  useEffect(() => {
    if (isActive) {
      void loadProfiles();
    }
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [isActive]);

  const openNew = () => {
    setEditor(buildEditor());
    setSecretKeys([]);
    setSecretMasks({});
    setSecretSetText("");
    setSecretUnsetText("");
    setSecretClear(false);
    setDuplicateSourceProfileId("");
    setEditorErr("");
    setEditorOpen(true);
  };

  const openEdit = async (profile: ActorProfile) => {
    setEditor(buildEditor(profile));
    setSecretSetText("");
    setSecretUnsetText("");
    setSecretClear(false);
    setDuplicateSourceProfileId("");
    setEditorErr("");
    setEditorOpen(true);
    await loadProfileSecrets(String(profile.id || ""));
  };

  const openDuplicate = async (profile: ActorProfile) => {
    const sourceId = String(profile.id || "").trim();
    setEditor({
      ...buildEditor(profile),
      id: "",
      revision: 0,
      name: `${String(profile.name || "").trim()} Copy`.trim(),
    });
    setSecretKeys([]);
    setSecretMasks({});
    setSecretSetText("");
    setSecretUnsetText("");
    setSecretClear(false);
    setDuplicateSourceProfileId(sourceId);
    setEditorErr("");
    setEditorOpen(true);
    await loadProfileSecrets(sourceId);
  };

  const handleDelete = async (profile: ActorProfile) => {
    if (!window.confirm(t("actorProfiles.deleteConfirm", { name: profile.name || profile.id }))) return;
    setBusy(true);
    setErr("");
    try {
      const resp = await api.deleteActorProfile(profile.id);
      if (!resp.ok) {
        const code = String(resp.error?.code || "").trim();
        if (code === "profile_in_use") {
          const details = resp.error?.details as { usage?: ActorProfileUsage[] } | undefined;
          const usage = Array.isArray(details?.usage) ? details.usage : [];
          const lines = usage
            .slice(0, 6)
            .map((item) => {
              const gid = String(item.group_id || "").trim();
              const aid = String(item.actor_id || "").trim();
              if (!gid || !aid) return "";
              const gtitle = String(item.group_title || "").trim();
              const atitle = String(item.actor_title || "").trim();
              const groupLabel = gtitle ? `${gtitle} (${gid})` : gid;
              const actorLabel = atitle ? `${atitle} (${aid})` : aid;
              return `${t("actorProfiles.deleteInUseEntryGroup", { group: groupLabel })}\n  ${t("actorProfiles.deleteInUseEntryActor", { actor: actorLabel })}`;
            })
            .filter((line) => line.length > 0)
            .join("\n");
          const over = usage.length > 6 ? `\n${t("actorProfiles.deleteInUseMore", { count: usage.length - 6 })}` : "";
          const withUsage = lines ? `\n\n${lines}${over}` : "";
          const forceConfirm = window.confirm(
            `${t("actorProfiles.deleteInUseConfirm", { count: usage.length || Number(profile.usage_count || 0) })}${withUsage}\n\n${t(
              "actorProfiles.deleteInUseForce"
            )}`
          );
          if (!forceConfirm) return;
          const forceResp = await api.deleteActorProfile(profile.id, { forceDetach: true });
          if (!forceResp.ok) {
            setErr(forceResp.error?.message || t("actorProfiles.deleteFailed"));
            return;
          }
          await loadProfiles();
          // force-detach mutates actor.profile_id in one or more groups; refresh current group actors
          // so Edit Actor modal does not open from stale linked state.
          await refreshActors();
          return;
        }
        setErr(resp.error?.message || t("actorProfiles.deleteFailed"));
        return;
      }
      await loadProfiles();
      await refreshActors();
    } catch {
      setErr(t("actorProfiles.deleteFailed"));
    } finally {
      setBusy(false);
    }
  };

  const handleSave = async () => {
    const name = editor.name.trim();
    if (!name) {
      setEditorErr(t("actorProfiles.nameRequired"));
      return;
    }
    setEditorBusy(true);
    setEditorErr("");
    try {
      const setParsed = parsePrivateEnvSetText(secretSetText);
      if (!setParsed.ok) {
        setEditorErr(setParsed.error);
        return;
      }
      const unsetParsed = parsePrivateEnvUnsetText(secretUnsetText);
      if (!unsetParsed.ok) {
        setEditorErr(unsetParsed.error);
        return;
      }

      const payload: Record<string, unknown> = {
        id: editor.id || undefined,
        name,
        runtime: editor.runtime,
        runner: editor.runner,
        command: editorSupportsDefaultCommand && editor.useDefaultCommand ? "" : editor.command.trim(),
        submit: editor.submit,
        env: {},
      };
      if (
        editorSupportsDefaultCommand &&
        !editor.useDefaultCommand &&
        !String(payload.command || "").trim()
      ) {
        setEditorErr(t("actorProfiles.commandOverrideRequired"));
        return;
      }
      if (
        String(editor.runtime || "").trim() === "custom" &&
        editor.runner !== "headless" &&
        !String(payload.command || "").trim()
      ) {
        setEditorErr(t("actorProfiles.customRuntimeCommandRequired"));
        return;
      }
      const expectedRevision = editor.id ? editor.revision : undefined;
      const upsertResp = await api.upsertActorProfile(payload, expectedRevision);
      if (!upsertResp.ok) {
        setEditorErr(upsertResp.error?.message || t("actorProfiles.saveFailed"));
        return;
      }
      const profile = upsertResp.result?.profile;
      const profileId = String(profile?.id || "").trim();
      if (!profileId) {
        setEditorErr(t("actorProfiles.saveFailed"));
        return;
      }

      const copyFromProfileId = duplicateSourceProfileId.trim();
      if (copyFromProfileId && copyFromProfileId !== profileId) {
        const copyResp = await api.copyActorProfilePrivateEnvFromProfile(profileId, copyFromProfileId);
        if (!copyResp.ok) {
          setEditorErr(copyResp.error?.message || t("actorProfiles.saveSecretsFailed"));
          return;
        }
      }

      const hasSecretOps = secretClear || Object.keys(setParsed.setVars).length > 0 || unsetParsed.unsetKeys.length > 0;
      if (hasSecretOps) {
        const secretResp = await api.updateActorProfilePrivateEnv(profileId, setParsed.setVars, unsetParsed.unsetKeys, secretClear);
        if (!secretResp.ok) {
          setEditorErr(secretResp.error?.message || t("actorProfiles.saveSecretsFailed"));
          return;
        }
      }
      await loadProfiles();
      setDuplicateSourceProfileId("");
      setEditorOpen(false);
    } catch {
      setEditorErr(t("actorProfiles.saveFailed"));
    } finally {
      setEditorBusy(false);
    }
  };

  return (
    <div className="space-y-4">
      <div className="flex flex-wrap items-center gap-2">
        <input
          value={query}
          onChange={(e) => setQuery(e.target.value)}
          placeholder={t("actorProfiles.searchPlaceholder")}
          className={`${inputClass(isDark)} max-w-sm`}
        />
        <button className={primaryButtonClass(false)} onClick={openNew}>
          {t("actorProfiles.newProfile")}
        </button>
        <button
          className={`px-3 py-2 rounded-lg text-sm min-h-[44px] font-medium transition-colors ${
            isDark ? "bg-slate-800 hover:bg-slate-700 text-slate-200" : "bg-white hover:bg-gray-50 text-gray-800 border border-gray-200"
          }`}
          onClick={() => void loadProfiles()}
          disabled={busy}
        >
          {busy ? t("common:loading") : t("actorProfiles.refresh")}
        </button>
      </div>

      {err ? (
        <div className={`rounded-lg border px-3 py-2 text-sm ${isDark ? "border-rose-500/30 bg-rose-500/10 text-rose-300" : "border-rose-200 bg-rose-50 text-rose-700"}`}>
          {err}
        </div>
      ) : null}

      <div className="space-y-2">
        {filtered.map((profile) => (
          <div key={profile.id} className={cardClass(isDark)}>
            <div className="flex flex-col gap-2 sm:flex-row sm:items-center sm:justify-between">
              <div className="min-w-0">
                <div className={`text-sm font-semibold truncate ${isDark ? "text-slate-200" : "text-gray-800"}`}>
                  {profile.name || profile.id}
                </div>
                <div className={`mt-0.5 text-xs ${isDark ? "text-slate-400" : "text-gray-600"}`}>
                  <code>{profile.id}</code> · {RUNTIME_INFO[String(profile.runtime)]?.label || profile.runtime} · {profile.runner}
                </div>
                <div className={`mt-0.5 text-[11px] ${isDark ? "text-slate-500" : "text-gray-500"}`}>
                  {t("actorProfiles.usageCount", { count: Number(profile.usage_count || 0) })} · {t("actorProfiles.revision", { revision: Number(profile.revision || 0) })}
                </div>
              </div>
              <div className="flex flex-wrap gap-2">
                <button
                  onClick={() => void openEdit(profile)}
                  className={`px-3 py-2 rounded-lg text-sm min-h-[40px] ${isDark ? "bg-slate-800 hover:bg-slate-700 text-slate-200" : "bg-white hover:bg-gray-50 text-gray-700 border border-gray-200"}`}
                >
                  {t("common:edit")}
                </button>
                <button
                  onClick={() => void openDuplicate(profile)}
                  className={`px-3 py-2 rounded-lg text-sm min-h-[40px] ${isDark ? "bg-slate-800 hover:bg-slate-700 text-slate-200" : "bg-white hover:bg-gray-50 text-gray-700 border border-gray-200"}`}
                >
                  {t("actorProfiles.duplicate")}
                </button>
                <button
                  onClick={() => void handleDelete(profile)}
                  className={`px-3 py-2 rounded-lg text-sm min-h-[40px] ${isDark ? "bg-rose-900/40 hover:bg-rose-900/60 text-rose-200" : "bg-rose-50 hover:bg-rose-100 text-rose-700 border border-rose-200"}`}
                >
                  {t("common:delete")}
                </button>
              </div>
            </div>
          </div>
        ))}
        {!busy && filtered.length === 0 ? (
          <div className={`text-sm ${isDark ? "text-slate-500" : "text-gray-500"}`}>{t("actorProfiles.empty")}</div>
        ) : null}
      </div>

      {editorOpen ? (
        <div className={`fixed inset-0 z-[70] flex items-center justify-center p-3 ${isDark ? "bg-black/60" : "bg-black/40"}`}>
          <div className={`w-full max-w-xl max-h-[90vh] overflow-y-auto rounded-2xl border shadow-2xl ${isDark ? "border-slate-700 bg-slate-900" : "border-gray-200 bg-white"}`}>
            <div className={`sticky top-0 px-5 py-4 border-b ${isDark ? "border-slate-700 bg-slate-900" : "border-gray-200 bg-white"}`}>
              <div className={`text-base font-semibold ${isDark ? "text-slate-100" : "text-gray-900"}`}>
                {editor.id ? t("actorProfiles.editTitle") : t("actorProfiles.newTitle")}
              </div>
            </div>
            <div className="p-5 space-y-4">
              {editorErr ? (
                <div className={`rounded-lg border px-3 py-2 text-sm ${isDark ? "border-rose-500/30 bg-rose-500/10 text-rose-300" : "border-rose-200 bg-rose-50 text-rose-700"}`}>
                  {editorErr}
                </div>
              ) : null}

              <div>
                <label className={labelClass(isDark)}>{t("actorProfiles.name")}</label>
                <input
                  value={editor.name}
                  onChange={(e) => setEditor((prev) => ({ ...prev, name: e.target.value }))}
                  className={inputClass(isDark)}
                />
              </div>

              <div className="grid grid-cols-1 sm:grid-cols-2 gap-3">
                <div>
                  <label className={labelClass(isDark)}>{t("actorProfiles.runtime")}</label>
                  <select
                    value={editor.runtime}
                    onChange={(e) => {
                      const nextRuntime = String(e.target.value || "");
                      setEditor((prev) => {
                        const supportsDefault = supportsRuntimeDefaultCommand(nextRuntime, prev.runner);
                        return {
                          ...prev,
                          runtime: nextRuntime,
                          useDefaultCommand: supportsDefault ? prev.useDefaultCommand : false,
                          command: supportsDefault && prev.useDefaultCommand ? "" : prev.command,
                        };
                      });
                    }}
                    className={inputClass(isDark)}
                  >
                    {SUPPORTED_RUNTIMES.map((rt) => (
                      <option key={rt} value={rt}>{RUNTIME_INFO[rt]?.label || rt}</option>
                    ))}
                  </select>
                </div>
                <div>
                  <label className={labelClass(isDark)}>{t("actorProfiles.runner")}</label>
                  <select
                    value={editor.runner}
                    onChange={(e) => {
                      const nextRunner = e.target.value === "headless" ? "headless" : "pty";
                      setEditor((prev) => {
                        const supportsDefault = supportsRuntimeDefaultCommand(prev.runtime, nextRunner);
                        return {
                          ...prev,
                          runner: nextRunner,
                          useDefaultCommand: supportsDefault ? prev.useDefaultCommand : false,
                          command: supportsDefault && prev.useDefaultCommand ? "" : prev.command,
                        };
                      });
                    }}
                    className={inputClass(isDark)}
                  >
                    <option value="pty">PTY</option>
                    <option value="headless">Headless</option>
                  </select>
                </div>
              </div>

              <div>
                <label className={labelClass(isDark)}>{t("actorProfiles.commandOverrideOptional")}</label>
                {editorSupportsDefaultCommand ? (
                  <label className={`inline-flex items-center gap-2 text-xs mb-2 ${isDark ? "text-slate-300" : "text-gray-700"}`}>
                    <input
                      type="checkbox"
                      checked={editor.useDefaultCommand}
                      onChange={(e) => {
                        const checked = e.target.checked;
                        setEditor((prev) => ({
                          ...prev,
                          useDefaultCommand: checked,
                          command: checked ? "" : prev.command,
                        }));
                      }}
                    />
                    {t("actorProfiles.useRuntimeDefaultCommand")}
                  </label>
                ) : null}
                {!editorSupportsDefaultCommand || !editor.useDefaultCommand ? (
                  <input
                    value={editor.command}
                    onChange={(e) => setEditor((prev) => ({ ...prev, command: e.target.value }))}
                    className={`${inputClass(isDark)} font-mono`}
                    placeholder={editorDefaultCommand || "codex"}
                  />
                ) : null}
                {editorSupportsDefaultCommand && editorDefaultCommand ? (
                  <div className={`text-[10px] mt-1 ${isDark ? "text-slate-500" : "text-gray-500"}`}>
                    {editor.useDefaultCommand ? t("actorProfiles.usingRuntimeDefaultCommand") : t("actorProfiles.default")}{" "}
                    <code className={`px-1 rounded ${isDark ? "bg-slate-800" : "bg-gray-100"}`}>{editorDefaultCommand}</code>
                  </div>
                ) : null}
              </div>

              <div>
                <label className={labelClass(isDark)}>{t("actorProfiles.submit")}</label>
                <select
                  value={editor.submit}
                  onChange={(e) => setEditor((prev) => ({ ...prev, submit: (e.target.value as "enter" | "newline" | "none") }))}
                  className={inputClass(isDark)}
                >
                  <option value="enter">Enter</option>
                  <option value="newline">Newline</option>
                  <option value="none">None</option>
                </select>
              </div>

              <div className={cardClass(isDark)}>
                <div className={`text-sm font-semibold ${isDark ? "text-slate-200" : "text-gray-800"}`}>{t("actorProfiles.env")}</div>
                <div className={`text-xs mt-1 ${isDark ? "text-slate-500" : "text-gray-500"}`}>{t("actorProfiles.envHint")}</div>
                {duplicateSourceProfileId ? (
                  <div className={`text-xs mt-1 ${isDark ? "text-slate-400" : "text-gray-600"}`}>
                    {t("actorProfiles.duplicateSecretsHint", { source: duplicateSourceLabel })}
                  </div>
                ) : null}
                {secretKeys.length ? (
                  <div className="mt-2 flex flex-wrap gap-1.5">
                    {secretKeys.map((key) => (
                      <span
                        key={key}
                        title={secretMasks[key] ? `${key}=${secretMasks[key]}` : key}
                        className={`px-2 py-0.5 rounded text-[11px] ${isDark ? "bg-slate-800 text-slate-200" : "bg-gray-100 text-gray-700"}`}
                      >
                        {key}
                      </span>
                    ))}
                  </div>
                ) : (
                  <div className={`mt-2 text-xs ${isDark ? "text-slate-500" : "text-gray-500"}`}>{t("actorProfiles.noSecrets")}</div>
                )}

                <div className="mt-3">
                  <label className={labelClass(isDark)}>{t("actorProfiles.setSecrets")}</label>
                  <textarea
                    value={secretSetText}
                    onChange={(e) => setSecretSetText(e.target.value)}
                    className={`${inputClass(isDark)} min-h-[90px] font-mono`}
                    placeholder={t("actorProfiles.setSecretsPlaceholder")}
                  />
                </div>
                <div className="mt-3">
                  <label className={labelClass(isDark)}>{t("actorProfiles.unsetSecrets")}</label>
                  <textarea
                    value={secretUnsetText}
                    onChange={(e) => setSecretUnsetText(e.target.value)}
                    className={`${inputClass(isDark)} min-h-[70px] font-mono`}
                    placeholder={t("actorProfiles.unsetSecretsPlaceholder")}
                  />
                </div>
                <label className={`mt-3 inline-flex items-center gap-2 text-sm ${isDark ? "text-slate-300" : "text-gray-700"}`}>
                  <input type="checkbox" checked={secretClear} onChange={(e) => setSecretClear(e.target.checked)} />
                  {t("actorProfiles.clearSecrets")}
                </label>
              </div>
            </div>
            <div className={`sticky bottom-0 px-5 py-4 border-t flex justify-end gap-2 ${isDark ? "border-slate-700 bg-slate-900" : "border-gray-200 bg-white"}`}>
              <button
                onClick={() => {
                  setDuplicateSourceProfileId("");
                  setEditorOpen(false);
                }}
                className={`px-3 py-2 rounded-lg text-sm min-h-[44px] ${isDark ? "bg-slate-800 hover:bg-slate-700 text-slate-200" : "bg-white hover:bg-gray-50 text-gray-700 border border-gray-200"}`}
              >
                {t("common:cancel")}
              </button>
              <button
                onClick={() => void handleSave()}
                disabled={editorBusy}
                className={primaryButtonClass(editorBusy)}
              >
                {editorBusy ? t("common:saving") : t("common:save")}
              </button>
            </div>
          </div>
        </div>
      ) : null}
    </div>
  );
}
