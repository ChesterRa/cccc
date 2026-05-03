import React, { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { useTranslation } from "react-i18next";

import * as api from "../../../services/api";
import type { GroupPromptInfo } from "../../../services/api";
import type { AssistantStateResult, BuiltinAssistant } from "../../../types";
import {
  DEFAULT_SERVICE_MODEL_ID,
  STREAMING_ASR_RUNTIME_ID,
} from "../../../pages/chat/voice-secretary/voiceServiceModelRuntime";
import { parseHelpMarkdown, updatePetHelpNote, updateVoiceSecretaryHelpNote } from "../../../utils/helpMarkdown";
import { getDefaultPetPersonaSeed } from "../../../utils/rolePresets";
import { GroupCombobox } from "../../GroupCombobox";
import { BodyPortal } from "../../ui/BodyPortal";
import { resolveLocalAsrModels } from "./assistantsLocalAsrModels";
import {
  inputClass,
  labelClass,
  primaryButtonClass,
  secondaryButtonClass,
  settingsDialogBodyClass,
  settingsDialogPanelClass,
  settingsWorkspaceActionBarClass,
  settingsWorkspaceBodyClass,
  settingsWorkspaceHeaderClass,
  settingsWorkspacePanelClass,
  settingsWorkspaceShellClass,
  settingsWorkspaceSoftPanelClass,
} from "./types";

interface AssistantsTabProps {
  isDark: boolean;
  groupId?: string;
  isActive: boolean;
  petEnabled: boolean;
  busy: boolean;
  onUpdatePetEnabled?: (enabled: boolean) => Promise<boolean | void>;
}

const VOICE_BACKENDS = [
  "browser_asr",
  "assistant_service_local_asr",
  "external_provider_asr",
];
const VOICE_AVAILABLE_BACKENDS = new Set(["browser_asr", "assistant_service_local_asr"]);

const VOICE_RECOMMENDED_QUIET_SECONDS = 5;
const VOICE_MIN_QUIET_SECONDS = 1;
const VOICE_MAX_QUIET_SECONDS = 60;
const VOICE_RECOMMENDED_MAX_WINDOW_SECONDS = 120;
const VOICE_MIN_MAX_WINDOW_SECONDS = 10;
const VOICE_MAX_MAX_WINDOW_SECONDS = 300;
const DIARIZATION_MODEL_ID = "sherpa_onnx_diarization_pyannote_3dspeaker_zh";
const LEGACY_DEFAULT_SERVICE_MODEL_IDS = new Set([""]);

const DEFAULT_VOICE_SECRETARY_GUIDANCE = [
  "- Keep working documents useful: synthesize decisions, action items, requirements, risks, and open questions; do not dump raw transcript.",
  "- Treat safe secretary-scope work as yours: summarize, structure, compare, draft, lightly inspect available context, and refine documents.",
  "- Hand off only non-secretary work such as code/test/deploy, actor management, risky commands, or explicit peer/foreman coordination.",
  "- Use `document_path` as the document identity. Create separate markdown documents for separate deliverables.",
  "- Preserve uncertainty and ASR-risk terms. For fragmented audio, write a best-effort rolling summary instead of refusing.",
].join("\n");

type AssistantPromptBlock = "pet" | "voice_secretary";

function findAssistant(state: AssistantStateResult | null, assistantId: string): BuiltinAssistant | null {
  if (!state) return null;
  const byId = state.assistants_by_id || {};
  if (byId[assistantId]) return byId[assistantId];
  return (state.assistants || []).find((assistant) => assistant.assistant_id === assistantId) || null;
}

function readStringConfig(assistant: BuiltinAssistant | null, key: string, fallback: string): string {
  const value = assistant?.config?.[key];
  return typeof value === "string" && value.trim() ? value.trim() : fallback;
}

function normalizeVoiceRecognitionLanguageForBackend(language: string, backend: string): string {
  const configured = String(language || "").trim() || "mixed";
  return String(backend || "").trim() === "browser_asr" && configured === "mixed" ? "auto" : configured;
}

function clampNumber(value: number, min: number, max: number): number {
  if (!Number.isFinite(value)) return min;
  return Math.min(Math.max(value, min), max);
}

function readNumberConfig(assistant: BuiltinAssistant | null, key: string, fallback: number, min: number, max: number): number {
  const raw = assistant?.config?.[key];
  const value = typeof raw === "number" ? raw : typeof raw === "string" ? Number(raw) : fallback;
  return clampNumber(value, min, max);
}

function formatModelSize(bytes: number | undefined): string {
  const value = Number(bytes || 0);
  if (!Number.isFinite(value) || value <= 0) return "";
  if (value >= 1024 * 1024 * 1024) return `${(value / (1024 * 1024 * 1024)).toFixed(1)} GiB`;
  if (value >= 1024 * 1024) return `${(value / (1024 * 1024)).toFixed(1)} MiB`;
  if (value >= 1024) return `${Math.round(value / 1024)} KiB`;
  return `${Math.round(value)} B`;
}

function recordFromUnknown(value: unknown): Record<string, unknown> {
  return value && typeof value === "object" && !Array.isArray(value) ? (value as Record<string, unknown>) : {};
}

function resolvePetPersonaDraft(savedPetPersona: string): string {
  const saved = String(savedPetPersona || "").trim();
  return saved || getDefaultPetPersonaSeed();
}

function resolveVoiceSecretaryGuidanceDraft(savedGuidance: string): string {
  const saved = String(savedGuidance || "").trim();
  return saved || DEFAULT_VOICE_SECRETARY_GUIDANCE;
}

function promptDraftDirty(savedText: string, draft: string, loaded: boolean, fallback: string): boolean {
  const draftText = String(draft || "");
  if (!loaded && !draftText.trim()) return false;
  return draftText !== (String(savedText || "").trim() || fallback);
}

function StatusPill({ children, tone }: { children: React.ReactNode; tone: "on" | "off" | "info" }) {
  const classes =
    tone === "on"
      ? "border border-emerald-600/15 bg-emerald-50 text-emerald-800 dark:border-emerald-400/18 dark:bg-emerald-400/10 dark:text-emerald-200"
      : tone === "off"
        ? "border border-slate-500/10 bg-slate-500/8 text-[var(--color-text-muted)]"
        : "border border-black/10 bg-[rgb(245,245,245)] text-[rgb(35,36,37)] dark:border-white/12 dark:bg-white/[0.08] dark:text-white";
  return (
    <span className={`inline-flex items-center rounded-full px-2.5 py-1 text-[11px] font-medium shadow-[inset_0_1px_0_rgba(255,255,255,0.55)] ${classes}`}>
      {children}
    </span>
  );
}

function localVoicePanelClass(isDark: boolean) {
  return `rounded-xl border p-4 ${
    isDark
      ? "border-white/10 bg-white/[0.035]"
      : "border-slate-200/80 bg-white/70"
  }`;
}

function localVoiceModelCardClass(isDark: boolean) {
  return `rounded-lg border p-3 ${
    isDark
      ? "border-white/10 bg-black/10"
      : "border-slate-200/75 bg-slate-50/70"
  }`;
}

function AssistantsFeedbackToast({ error, notice }: { error: string; notice: string }) {
  const message = error || notice;
  if (!message) return null;

  const isError = Boolean(error);
  const classes = isError
    ? "border-rose-500/25 bg-rose-50 text-rose-700 shadow-rose-950/10 dark:bg-rose-500/15 dark:text-rose-200"
    : "border-emerald-500/25 bg-emerald-50 text-emerald-700 shadow-emerald-950/10 dark:bg-emerald-500/15 dark:text-emerald-200";

  return (
    <div
      role={isError ? "alert" : "status"}
      aria-live={isError ? "assertive" : "polite"}
      className={`pointer-events-none fixed bottom-5 right-5 z-[80] max-w-[min(28rem,calc(100vw-2.5rem))] rounded-xl border px-3 py-2 text-xs leading-5 shadow-lg backdrop-blur ${classes}`}
    >
      {message}
    </div>
  );
}

function AssistantSwitch({
  checked,
  disabled,
  label,
  hint,
  onChange,
}: {
  checked: boolean;
  disabled?: boolean;
  label: string;
  hint?: string;
  onChange: (checked: boolean) => void;
}) {
  return (
    <label className={`inline-flex select-none items-center justify-end gap-3 ${disabled ? "cursor-not-allowed opacity-60" : "cursor-pointer"}`}>
      <span className="min-w-0 text-right">
        <span className="block text-xs font-medium text-[var(--color-text-secondary)]">{label}</span>
        {hint ? <span className="mt-1 block text-[11px] leading-5 text-[var(--color-text-muted)]">{hint}</span> : null}
      </span>
      <input
        type="checkbox"
        role="switch"
        checked={checked}
        disabled={disabled}
        onChange={(event) => onChange(event.target.checked)}
        className="sr-only"
      />
      <span
        aria-hidden="true"
        className={`relative inline-flex h-7 w-12 shrink-0 items-center rounded-full border transition-colors ${
          checked
            ? "border-emerald-500 bg-emerald-500"
            : "border-[var(--glass-border-subtle)] bg-[var(--color-bg-secondary)]"
        } ${disabled ? "opacity-50" : ""}`}
      >
        <span
          className={`absolute left-0.5 h-6 w-6 rounded-full bg-white shadow-sm transition-transform ${
            checked ? "translate-x-5" : "translate-x-0"
          }`}
        />
      </span>
    </label>
  );
}

function SettingsBlock({
  isDark,
  title,
  hint,
  children,
}: {
  isDark: boolean;
  title: string;
  hint?: string;
  children: React.ReactNode;
}) {
  return (
    <section className={settingsWorkspacePanelClass(isDark)}>
      <div>
        <div className="text-sm font-semibold text-[var(--color-text-primary)]">{title}</div>
        {hint ? <p className="mt-1 text-[11px] leading-5 text-[var(--color-text-muted)]">{hint}</p> : null}
      </div>
      <div className="mt-4">{children}</div>
    </section>
  );
}

function AssistantPromptEditor({
  isDark,
  title,
  hint,
  path,
  value,
  placeholder,
  busy,
  error,
  notice,
  hasUnsaved,
  reloadLabel,
  discardLabel,
  saveLabel,
  expandLabel,
  expanded,
  onReload,
  onDiscard,
  onSave,
  onExpand,
  onChange,
}: {
  isDark: boolean;
  title: string;
  hint: string;
  path?: string;
  value: string;
  placeholder: string;
  busy: boolean;
  error: string;
  notice: string;
  hasUnsaved: boolean;
  reloadLabel: string;
  discardLabel: string;
  saveLabel: string;
  expandLabel?: string;
  expanded?: boolean;
  onReload: () => void;
  onDiscard: () => void;
  onSave: () => void;
  onExpand?: () => void;
  onChange: (value: string) => void;
}) {
  return (
    <div className={`${expanded ? "flex h-full min-h-0 flex-col" : settingsWorkspaceShellClass(isDark)}`}>
      <div className={expanded ? "flex flex-wrap items-start justify-between gap-2" : settingsWorkspaceHeaderClass(isDark)}>
        <div className="min-w-0">
          <div className="text-sm font-medium text-[var(--color-text-primary)]">{title}</div>
          <p className="mt-1 text-[11px] leading-5 text-[var(--color-text-muted)]">{hint}</p>
          {path ? (
            <p className="mt-2 break-all font-mono text-[11px] leading-5 text-[var(--color-text-muted)]">{path}</p>
          ) : null}
        </div>
        <div className="flex shrink-0 items-center gap-2">
          {!expanded && onExpand ? (
            <button type="button" onClick={onExpand} disabled={busy} className={secondaryButtonClass("sm")}>
              {expandLabel}
            </button>
          ) : null}
          <button type="button" onClick={onReload} disabled={busy} className={secondaryButtonClass("sm")}>
            {reloadLabel}
          </button>
        </div>
      </div>

      <div className={`${expanded ? "mt-3 min-h-0 flex flex-1 flex-col" : settingsWorkspaceBodyClass}`}>
        {error ? (
          <div className="rounded-xl border border-rose-500/25 bg-rose-500/10 px-3 py-2 text-xs text-rose-700 dark:text-rose-300">
            {error}
          </div>
        ) : null}
        {notice ? (
          <div className="rounded-xl border border-emerald-500/25 bg-emerald-500/10 px-3 py-2 text-xs text-emerald-700 dark:text-emerald-300">
            {notice}
          </div>
        ) : null}

        <div className={`${settingsWorkspaceSoftPanelClass(isDark)} ${
          expanded ? "min-h-0 flex flex-1 flex-col" : ""
        }`}>
          <textarea
            value={value}
            onChange={(event) => onChange(event.target.value)}
            disabled={busy}
            placeholder={placeholder}
            className={`${inputClass(isDark)} resize-y font-mono text-[12px] leading-6 ${
              expanded ? "min-h-[560px] flex-1" : "min-h-[28rem] lg:min-h-[32rem]"
            }`}
            spellCheck={false}
          />
        </div>
      </div>

      <div className={expanded ? "mt-3 flex flex-wrap justify-end gap-2" : settingsWorkspaceActionBarClass(isDark)}>
        <button type="button" onClick={onDiscard} disabled={busy || !hasUnsaved} className={secondaryButtonClass("sm")}>
          {discardLabel}
        </button>
        <button type="button" onClick={onSave} disabled={busy || !hasUnsaved} className={primaryButtonClass(busy)}>
          {saveLabel}
        </button>
      </div>
    </div>
  );
}

export function AssistantsTab({
  isDark,
  groupId,
  isActive,
  petEnabled,
  busy,
  onUpdatePetEnabled,
}: AssistantsTabProps) {
  const { t } = useTranslation("settings");
  const loadSeq = useRef(0);
  const visibleLoadCount = useRef(0);

  const [assistantState, setAssistantState] = useState<AssistantStateResult | null>(null);
  const [loadBusy, setLoadBusy] = useState(false);
  const [voiceSaveBusy, setVoiceSaveBusy] = useState(false);
  const [petSaveBusy, setPetSaveBusy] = useState(false);
  const [error, setError] = useState("");
  const [notice, setNotice] = useState("");

  const [voiceEnabled, setVoiceEnabled] = useState(false);
  const [recognitionBackend, setRecognitionBackend] = useState("browser_asr");
  const [voiceQuietWindowSeconds, setVoiceQuietWindowSeconds] = useState(VOICE_RECOMMENDED_QUIET_SECONDS);
  const [voiceMaxWindowSeconds, setVoiceMaxWindowSeconds] = useState(VOICE_RECOMMENDED_MAX_WINDOW_SECONDS);
  const [serviceRuntimeInstallBusy, setServiceRuntimeInstallBusy] = useState(false);
  const [diarizationModelInstallBusy, setDiarizationModelInstallBusy] = useState(false);
  const [localAsrMaintenanceBusy, setLocalAsrMaintenanceBusy] = useState(false);

  const [assistantHelpPrompt, setAssistantHelpPrompt] = useState<GroupPromptInfo | null>(null);
  const [petPersonaDraft, setPetPersonaDraft] = useState("");
  const [voiceSecretaryGuidanceDraft, setVoiceSecretaryGuidanceDraft] = useState("");
  const [assistantPromptBusy, setAssistantPromptBusy] = useState(false);
  const [assistantPromptError, setAssistantPromptError] = useState("");
  const [assistantPromptNotice, setAssistantPromptNotice] = useState("");
  const [assistantPromptFeedbackBlock, setAssistantPromptFeedbackBlock] = useState<AssistantPromptBlock | "">("");
  const [expandedPromptBlock, setExpandedPromptBlock] = useState<AssistantPromptBlock | null>(null);

  const voiceAssistant = useMemo(
    () => findAssistant(assistantState, "voice_secretary"),
    [assistantState],
  );
  const petAssistant = useMemo(
    () => findAssistant(assistantState, "pet"),
    [assistantState],
  );
  const effectivePetEnabled = Boolean(petAssistant?.enabled ?? petEnabled);

  const syncVoiceDraft = useCallback((state: AssistantStateResult | null) => {
    const voice = findAssistant(state, "voice_secretary");
    const backend = readStringConfig(voice, "recognition_backend", "browser_asr");
    setVoiceEnabled(Boolean(voice?.enabled));
    setRecognitionBackend(backend || "browser_asr");
    setVoiceQuietWindowSeconds(readNumberConfig(
      voice,
      "auto_document_quiet_ms",
      VOICE_RECOMMENDED_QUIET_SECONDS * 1000,
      VOICE_MIN_QUIET_SECONDS * 1000,
      VOICE_MAX_QUIET_SECONDS * 1000,
    ) / 1000);
    setVoiceMaxWindowSeconds(readNumberConfig(
      voice,
      "auto_document_max_window_seconds",
      VOICE_RECOMMENDED_MAX_WINDOW_SECONDS,
      VOICE_MIN_MAX_WINDOW_SECONDS,
      VOICE_MAX_MAX_WINDOW_SECONDS,
    ));
  }, []);

  const loadAssistants = useCallback(async (opts?: { quiet?: boolean }) => {
    const gid = String(groupId || "").trim();
    if (!gid) return;
    const seq = ++loadSeq.current;
    const showBusy = !opts?.quiet;
    if (showBusy) {
      visibleLoadCount.current += 1;
      setLoadBusy(true);
    }
    setError("");
    try {
      const resp = await api.fetchAssistantState(gid);
      if (seq !== loadSeq.current) return;
      if (!resp.ok) {
        setAssistantState(null);
        setError(resp.error?.message || t("assistants.loadFailed"));
        return;
      }
      setAssistantState(resp.result);
      syncVoiceDraft(resp.result);
    } catch {
      if (seq === loadSeq.current) {
        setAssistantState(null);
        setError(t("assistants.loadFailed"));
      }
    } finally {
      if (showBusy) {
        visibleLoadCount.current = Math.max(0, visibleLoadCount.current - 1);
        if (visibleLoadCount.current === 0) setLoadBusy(false);
      }
    }
  }, [groupId, syncVoiceDraft, t]);

  const loadAssistantGuidance = useCallback(async (opts?: { force?: boolean }) => {
    const gid = String(groupId || "").trim();
    if (!gid) return null;
    if (!opts?.force && assistantHelpPrompt) return assistantHelpPrompt;
    setAssistantPromptBusy(true);
    setAssistantPromptError("");
    setAssistantPromptFeedbackBlock("");
    try {
      const resp = await api.fetchGroupPrompts(gid);
      if (!resp.ok) {
        setAssistantHelpPrompt(null);
        setAssistantPromptError(resp.error?.message || t("assistants.assistantGuidanceLoadFailed"));
        return null;
      }
      const nextHelp = resp.result?.help ?? null;
      if (!nextHelp) {
        setAssistantHelpPrompt(null);
        setAssistantPromptError(t("assistants.assistantGuidanceLoadFailed"));
        return null;
      }
      const parsed = parseHelpMarkdown(String(nextHelp.content || ""));
      setAssistantHelpPrompt(nextHelp);
      setPetPersonaDraft(resolvePetPersonaDraft(parsed.pet));
      setVoiceSecretaryGuidanceDraft(resolveVoiceSecretaryGuidanceDraft(parsed.voiceSecretary));
      return nextHelp;
    } catch {
      setAssistantHelpPrompt(null);
      setAssistantPromptError(t("assistants.assistantGuidanceLoadFailed"));
      return null;
    } finally {
      setAssistantPromptBusy(false);
    }
  }, [assistantHelpPrompt, groupId, t]);

  useEffect(() => {
    if (!isActive) return;
    void loadAssistants();
    void loadAssistantGuidance();
  }, [isActive, loadAssistants, loadAssistantGuidance]);

  const saveVoiceSettings = async (overrides?: {
    enabled?: boolean;
    backend?: string;
    quietSeconds?: number;
    maxWindowSeconds?: number;
  }) => {
    const gid = String(groupId || "").trim();
    if (!gid) return false;
    setVoiceSaveBusy(true);
    setError("");
    setNotice("");
    try {
      const nextEnabled = typeof overrides?.enabled === "boolean" ? overrides.enabled : voiceEnabled;
      const nextBackend = String((overrides?.backend ?? recognitionBackend) || "browser_asr").trim() || "browser_asr";
      const nextRecognitionLanguage = normalizeVoiceRecognitionLanguageForBackend(
        readStringConfig(voiceAssistant, "recognition_language", "mixed"),
        nextBackend,
      );
      const quietSeconds = clampNumber(
        Number(overrides?.quietSeconds ?? voiceQuietWindowSeconds),
        VOICE_MIN_QUIET_SECONDS,
        VOICE_MAX_QUIET_SECONDS,
      );
      const maxWindowSeconds = clampNumber(
        Number(overrides?.maxWindowSeconds ?? voiceMaxWindowSeconds),
        VOICE_MIN_MAX_WINDOW_SECONDS,
        VOICE_MAX_MAX_WINDOW_SECONDS,
      );
      const resp = await api.updateAssistantSettings(gid, "voice_secretary", {
        enabled: nextEnabled,
        config: {
          capture_mode: nextBackend === "assistant_service_local_asr" ? "service" : "browser",
          recognition_backend: nextBackend,
          recognition_language: nextRecognitionLanguage,
          auto_document_enabled: true,
          auto_document_quiet_ms: Math.round(quietSeconds * 1000),
          auto_document_max_window_seconds: Math.round(maxWindowSeconds),
          document_default_dir: "docs/voice-secretary",
          service_model_id: "",
          tts_enabled: false,
        },
        by: "user",
      });
      if (!resp.ok) {
        setError(resp.error?.message || t("assistants.saveFailed"));
        return false;
      }
      setVoiceEnabled(nextEnabled);
      setRecognitionBackend(nextBackend);
      setVoiceQuietWindowSeconds(quietSeconds);
      setVoiceMaxWindowSeconds(maxWindowSeconds);
      setNotice(t("assistants.voiceSaved"));
      await loadAssistants({ quiet: true });
      return true;
    } catch {
      setError(t("assistants.saveFailed"));
      return false;
    } finally {
      setVoiceSaveBusy(false);
    }
  };

  const resetVoiceBatching = async () => {
    const previousQuiet = voiceQuietWindowSeconds;
    const previousMaxWindow = voiceMaxWindowSeconds;
    setVoiceQuietWindowSeconds(VOICE_RECOMMENDED_QUIET_SECONDS);
    setVoiceMaxWindowSeconds(VOICE_RECOMMENDED_MAX_WINDOW_SECONDS);
    const ok = await saveVoiceSettings({
      quietSeconds: VOICE_RECOMMENDED_QUIET_SECONDS,
      maxWindowSeconds: VOICE_RECOMMENDED_MAX_WINDOW_SECONDS,
    });
    if (!ok) {
      setVoiceQuietWindowSeconds(previousQuiet);
      setVoiceMaxWindowSeconds(previousMaxWindow);
    }
  };

  const toggleVoiceEnabled = async (nextEnabled: boolean) => {
    const previous = voiceEnabled;
    setVoiceEnabled(nextEnabled);
    const ok = await saveVoiceSettings({ enabled: nextEnabled });
    if (!ok) setVoiceEnabled(previous);
  };

  const togglePet = async (nextEnabled?: boolean) => {
    if (!onUpdatePetEnabled) return;
    setPetSaveBusy(true);
    setError("");
    setNotice("");
    const requestedEnabled = typeof nextEnabled === "boolean" ? nextEnabled : !effectivePetEnabled;
    try {
      const ok = await onUpdatePetEnabled(requestedEnabled);
      if (ok === false) {
        setError(t("assistants.petSaveFailed"));
        return;
      }
      setNotice(requestedEnabled ? t("assistants.petEnabled") : t("assistants.petDisabled"));
      await loadAssistants({ quiet: true });
    } catch {
      setError(t("assistants.petSaveFailed"));
    } finally {
      setPetSaveBusy(false);
    }
  };

  const saveAssistantGuidance = async (block: AssistantPromptBlock) => {
    const gid = String(groupId || "").trim();
    if (!gid) return;
    setAssistantPromptBusy(true);
    setAssistantPromptError("");
    setAssistantPromptNotice("");
    setAssistantPromptFeedbackBlock(block);
    try {
      const currentHelp = assistantHelpPrompt ?? await loadAssistantGuidance({ force: true });
      if (!currentHelp) return;
      setAssistantPromptFeedbackBlock(block);
      const currentContent = String(currentHelp.content || "");
      const parsed = parseHelpMarkdown(currentContent);
      const actorOrder = Object.keys(parsed.actorNotes);
      const nextContent =
        block === "pet"
          ? updatePetHelpNote(currentContent, petPersonaDraft, actorOrder)
          : updateVoiceSecretaryHelpNote(currentContent, voiceSecretaryGuidanceDraft, actorOrder);
      const resp = await api.updateGroupPrompt(gid, "help", nextContent, {
        editorMode: "structured",
        changedBlocks: [block],
      });
      if (!resp.ok) {
        setAssistantPromptError(resp.error?.message || t("assistants.assistantGuidanceSaveFailed"));
        return;
      }
      const nextHelp = resp.result;
      const nextParsed = parseHelpMarkdown(String(nextHelp.content || ""));
      setAssistantHelpPrompt(nextHelp);
      setPetPersonaDraft(resolvePetPersonaDraft(nextParsed.pet));
      setVoiceSecretaryGuidanceDraft(resolveVoiceSecretaryGuidanceDraft(nextParsed.voiceSecretary));
      setAssistantPromptNotice(
        block === "pet" ? t("assistants.petPersonaSaved") : t("assistants.voiceGuidanceSaved"),
      );
    } catch {
      setAssistantPromptError(t("assistants.assistantGuidanceSaveFailed"));
    } finally {
      setAssistantPromptBusy(false);
    }
  };

  const discardPetPersona = () => {
    const saved = assistantHelpPrompt ? parseHelpMarkdown(String(assistantHelpPrompt.content || "")).pet : "";
    setPetPersonaDraft(resolvePetPersonaDraft(saved));
    setAssistantPromptError("");
    setAssistantPromptNotice("");
    setAssistantPromptFeedbackBlock("");
  };

  const discardVoiceSecretaryGuidance = () => {
    const saved = assistantHelpPrompt ? parseHelpMarkdown(String(assistantHelpPrompt.content || "")).voiceSecretary : "";
    setVoiceSecretaryGuidanceDraft(resolveVoiceSecretaryGuidanceDraft(saved));
    setAssistantPromptError("");
    setAssistantPromptNotice("");
    setAssistantPromptFeedbackBlock("");
  };

  const backendOptions = VOICE_BACKENDS.includes(recognitionBackend)
    ? VOICE_BACKENDS
    : [recognitionBackend, ...VOICE_BACKENDS];
  const backendLabel = (backend: string) => t(`assistants.backends.${backend}`, { defaultValue: backend });
  const backendSelectable = (backend: string) => VOICE_AVAILABLE_BACKENDS.has(backend);
  const backendComboboxItems = backendOptions.map((backend) => ({
    value: backend,
    label: backendLabel(backend),
    disabled: !backendSelectable(backend),
  }));
  const currentBackendUnavailable = !backendSelectable(recognitionBackend);

  const voiceEnabledTone = voiceEnabled ? "on" : "off";
  const serviceHealth = recordFromUnknown(recordFromUnknown(voiceAssistant?.health).service);
  const serviceStatus = String(serviceHealth.status || (recognitionBackend === "assistant_service_local_asr" ? "not_started" : "")).trim();
  const serviceAlive = Boolean(serviceHealth.alive);
  const serviceRuntimesById = assistantState?.service_runtimes_by_id || {};
  const rawConfiguredServiceModelId = readStringConfig(voiceAssistant, "service_model_id", DEFAULT_SERVICE_MODEL_ID);
  const configuredServiceModelId = LEGACY_DEFAULT_SERVICE_MODEL_IDS.has(rawConfiguredServiceModelId)
    ? DEFAULT_SERVICE_MODEL_ID
    : rawConfiguredServiceModelId;
  const serviceModelsById = assistantState?.service_models_by_id || {};
  const { finalModel: finalServiceAsrModel, liveModel: liveServiceAsrModel } = resolveLocalAsrModels({
    configuredModelId: configuredServiceModelId,
    serviceModels: assistantState?.service_models || [],
    serviceModelsById,
  });
  const selectedServiceRuntimeId = String(
    liveServiceAsrModel?.runtime_id || finalServiceAsrModel?.runtime_id || STREAMING_ASR_RUNTIME_ID,
  ).trim() || STREAMING_ASR_RUNTIME_ID;
  const streamingRuntime = serviceRuntimesById[selectedServiceRuntimeId];
  const streamingRuntimeStatus = String(streamingRuntime?.status || "not_installed").trim() || "not_installed";
  const streamingRuntimeInstalling = streamingRuntimeStatus === "installing";
  const streamingRuntimeReady = streamingRuntimeStatus === "ready";
  const finalServiceAsrModelId = String(finalServiceAsrModel?.model_id || "").trim();
  const finalServiceAsrModelStatus = String(finalServiceAsrModel?.status || "not_installed").trim() || "not_installed";
  const finalServiceAsrModelInstalling = finalServiceAsrModelStatus === "downloading";
  const finalServiceAsrModelReady = finalServiceAsrModelStatus === "ready";
  const finalServiceAsrModelSize = formatModelSize(finalServiceAsrModel?.total_size_bytes);
  const liveServiceAsrModelId = String(liveServiceAsrModel?.model_id || "").trim();
  const liveServiceAsrModelStatus = String(liveServiceAsrModel?.status || "not_installed").trim() || "not_installed";
  const liveServiceAsrModelInstalling = liveServiceAsrModelStatus === "downloading";
  const liveServiceAsrModelReady = liveServiceAsrModelStatus === "ready";
  const liveServiceAsrModelSize = formatModelSize(liveServiceAsrModel?.total_size_bytes);
  const diarizationModel = serviceModelsById[DIARIZATION_MODEL_ID];
  const diarizationModelStatus = String(diarizationModel?.status || "not_installed").trim() || "not_installed";
  const diarizationModelInstalling = diarizationModelStatus === "downloading";
  const diarizationModelReady = diarizationModelStatus === "ready";
  const diarizationModelSize = formatModelSize(diarizationModel?.total_size_bytes);
  const diarizationModelDiskSize = formatModelSize(diarizationModel?.disk_usage_bytes);
  const localAsrInstalling = serviceRuntimeInstallBusy || streamingRuntimeInstalling || liveServiceAsrModelInstalling || finalServiceAsrModelInstalling;
  const localAsrReady = streamingRuntimeReady && liveServiceAsrModelReady && finalServiceAsrModelReady;
  const localAsrFailed = streamingRuntimeStatus === "failed" || liveServiceAsrModelStatus === "failed" || finalServiceAsrModelStatus === "failed";
  const localAsrStatusTone: "on" | "off" | "info" = localAsrReady ? "on" : localAsrFailed ? "off" : "info";
  const localAsrStatusLabel = localAsrInstalling
    ? t("assistants.localAsrInstalling", { defaultValue: "Installing" })
    : localAsrReady
      ? t("assistants.localAsrReady", { defaultValue: "Ready" })
      : localAsrFailed
        ? t("assistants.localAsrFailed", { defaultValue: "Needs repair" })
        : t("assistants.localAsrSetupNeeded", { defaultValue: "Setup needed" });
  const localAsrDiskUsage = formatModelSize(
    Number(streamingRuntime?.disk_usage_bytes || 0)
    + Number(liveServiceAsrModel?.disk_usage_bytes || 0)
    + Number(finalServiceAsrModel?.disk_usage_bytes || 0),
  );
  const selectedServiceModelInstalling = (
    serviceRuntimeInstallBusy
    || streamingRuntimeInstalling
    || liveServiceAsrModelInstalling
    || finalServiceAsrModelInstalling
    || diarizationModelInstallBusy
    || diarizationModelInstalling
    || localAsrMaintenanceBusy
  );
  const asrCommandConfigured = Boolean(
    serviceHealth.asr_command_configured
    || serviceHealth.asr_mock_configured
    || serviceHealth.managed_asr_command_configured,
  );
  const serviceLastError = recordFromUnknown(serviceHealth.last_error);
  const serviceLastErrorMessage = String(serviceLastError.message || "").trim();
  const serviceTone: "on" | "off" | "info" = recognitionBackend === "assistant_service_local_asr"
    ? serviceAlive
      ? "on"
      : asrCommandConfigured
        ? "info"
        : "off"
    : "info";
  const showServiceAsrDiagnostic =
    backendSelectable(recognitionBackend)
    && recognitionBackend === "assistant_service_local_asr"
    && (!asrCommandConfigured || Boolean(serviceLastErrorMessage));
  const showServiceModelControls =
    backendSelectable(recognitionBackend) && recognitionBackend === "assistant_service_local_asr";
  const localAsrModelIds = Array.from(new Set([liveServiceAsrModelId, finalServiceAsrModelId].filter(Boolean)));
  const canManageLocalAsr = localAsrModelIds.length > 0;

  const installLocalAsrModel = async (modelId: string): Promise<boolean> => {
    const gid = String(groupId || "").trim();
    if (!gid || !modelId) return false;
    const resp = await api.installVoiceAssistantModel(gid, {
      modelId,
      by: "user",
      background: true,
    });
    if (!resp.ok) {
      setError(resp.error?.message || t("assistants.streamingAsrModelInstallFailed", { defaultValue: "Failed to install ASR model." }));
      return false;
    }
    return true;
  };

  const installLocalAsrBundle = async () => {
    const gid = String(groupId || "").trim();
    if (!gid || !canManageLocalAsr) return;
    setServiceRuntimeInstallBusy(true);
    setError("");
    setNotice("");
    try {
      const saved = await saveVoiceSettings({ backend: "assistant_service_local_asr" });
      if (!saved) return;
      if (!streamingRuntimeReady) {
        const runtimeResp = await api.installVoiceAssistantRuntime(gid, {
          runtimeId: STREAMING_ASR_RUNTIME_ID,
          by: "user",
          background: true,
        });
        if (!runtimeResp.ok) {
          setError(runtimeResp.error?.message || t("assistants.streamingRuntimeInstallFailed"));
          return;
        }
      }
      if (!liveServiceAsrModelReady && liveServiceAsrModelId) {
        const installed = await installLocalAsrModel(liveServiceAsrModelId);
        if (!installed) return;
      }
      if (!finalServiceAsrModelReady && finalServiceAsrModelId) {
        const installed = await installLocalAsrModel(finalServiceAsrModelId);
        if (!installed) return;
      }
      setNotice(t("assistants.localAsrInstallStarted", { defaultValue: "Local ASR setup started." }));
      await loadAssistants({ quiet: true });
    } catch {
      setError(t("assistants.localAsrInstallFailed", { defaultValue: "Failed to install local ASR." }));
    } finally {
      setServiceRuntimeInstallBusy(false);
    }
  };

  const installDiarizationModel = async () => {
    const gid = String(groupId || "").trim();
    if (!gid) return;
    setDiarizationModelInstallBusy(true);
    setError("");
    setNotice("");
    try {
      const saved = await saveVoiceSettings({ backend: "assistant_service_local_asr" });
      if (!saved) return;
      const resp = await api.installVoiceAssistantModel(gid, {
        modelId: DIARIZATION_MODEL_ID,
        by: "user",
        background: true,
      });
      if (!resp.ok) {
        setError(resp.error?.message || t("assistants.diarizationModelInstallFailed", { defaultValue: "Failed to install speaker diarization model." }));
        return;
      }
      setNotice(t("assistants.diarizationModelInstallStarted", { defaultValue: "Speaker diarization model download started." }));
      await loadAssistants({ quiet: true });
    } catch {
      setError(t("assistants.diarizationModelInstallFailed", { defaultValue: "Failed to install speaker diarization model." }));
    } finally {
      setDiarizationModelInstallBusy(false);
    }
  };

  const removeLocalAsrBundle = async () => {
    const gid = String(groupId || "").trim();
    if (!gid || !canManageLocalAsr) return;
    if (!window.confirm(t("assistants.localAsrRemoveConfirm", { defaultValue: "Remove the local ASR engine and ASR models from this device?" }))) return;
    setLocalAsrMaintenanceBusy(true);
    setError("");
    setNotice("");
    try {
      for (const modelId of localAsrModelIds) {
        const modelResp = await api.removeVoiceAssistantModel(gid, {
          modelId,
          by: "user",
        });
        if (!modelResp.ok) {
          setError(modelResp.error?.message || t("assistants.localAsrRemoveFailed", { defaultValue: "Failed to remove local ASR." }));
          return;
        }
      }
      const runtimeResp = await api.removeVoiceAssistantRuntime(gid, {
        runtimeId: STREAMING_ASR_RUNTIME_ID,
        by: "user",
      });
      if (!runtimeResp.ok) {
        setError(runtimeResp.error?.message || t("assistants.localAsrRemoveFailed", { defaultValue: "Failed to remove local ASR." }));
        return;
      }
      setNotice(t("assistants.localAsrRemoved", { defaultValue: "Local ASR cache removed." }));
      await loadAssistants({ quiet: true });
    } catch {
      setError(t("assistants.localAsrRemoveFailed", { defaultValue: "Failed to remove local ASR." }));
    } finally {
      setLocalAsrMaintenanceBusy(false);
    }
  };

  const reinstallLocalAsrBundle = async () => {
    const gid = String(groupId || "").trim();
    if (!gid || !canManageLocalAsr) return;
    if (!window.confirm(t("assistants.localAsrReinstallConfirm", { defaultValue: "Reinstall the local ASR engine and ASR models?" }))) return;
    setLocalAsrMaintenanceBusy(true);
    setError("");
    setNotice("");
    try {
      for (const modelId of localAsrModelIds) {
        const modelRemove = await api.removeVoiceAssistantModel(gid, { modelId, by: "user" });
        if (!modelRemove.ok) {
          setError(modelRemove.error?.message || t("assistants.localAsrReinstallFailed", { defaultValue: "Failed to reinstall local ASR." }));
          return;
        }
      }
      const runtimeRemove = await api.removeVoiceAssistantRuntime(gid, { runtimeId: STREAMING_ASR_RUNTIME_ID, by: "user" });
      if (!runtimeRemove.ok) {
        setError(runtimeRemove.error?.message || t("assistants.localAsrReinstallFailed", { defaultValue: "Failed to reinstall local ASR." }));
        return;
      }
      const runtimeInstall = await api.installVoiceAssistantRuntime(gid, { runtimeId: STREAMING_ASR_RUNTIME_ID, by: "user", background: true });
      if (!runtimeInstall.ok) {
        setError(runtimeInstall.error?.message || t("assistants.localAsrReinstallFailed", { defaultValue: "Failed to reinstall local ASR." }));
        return;
      }
      for (const modelId of localAsrModelIds) {
        const modelInstall = await api.installVoiceAssistantModel(gid, { modelId, by: "user", background: true });
        if (!modelInstall.ok) {
          setError(modelInstall.error?.message || t("assistants.localAsrReinstallFailed", { defaultValue: "Failed to reinstall local ASR." }));
          return;
        }
      }
      setNotice(t("assistants.localAsrReinstallStarted", { defaultValue: "Local ASR reinstall started." }));
      await loadAssistants({ quiet: true });
    } catch {
      setError(t("assistants.localAsrReinstallFailed", { defaultValue: "Failed to reinstall local ASR." }));
    } finally {
      setLocalAsrMaintenanceBusy(false);
    }
  };

  const removeDiarizationModel = async () => {
    const gid = String(groupId || "").trim();
    if (!gid) return;
    if (!window.confirm(t("assistants.diarizationModelRemoveConfirm", { defaultValue: "Remove the speaker-label model from this device?" }))) return;
    setDiarizationModelInstallBusy(true);
    setError("");
    setNotice("");
    try {
      const resp = await api.removeVoiceAssistantModel(gid, {
        modelId: DIARIZATION_MODEL_ID,
        by: "user",
      });
      if (!resp.ok) {
        setError(resp.error?.message || t("assistants.diarizationModelRemoveFailed", { defaultValue: "Failed to remove speaker-label model." }));
        return;
      }
      setNotice(t("assistants.diarizationModelRemoved", { defaultValue: "Speaker-label model removed." }));
      await loadAssistants({ quiet: true });
    } catch {
      setError(t("assistants.diarizationModelRemoveFailed", { defaultValue: "Failed to remove speaker-label model." }));
    } finally {
      setDiarizationModelInstallBusy(false);
    }
  };

  const reinstallDiarizationModel = async () => {
    const gid = String(groupId || "").trim();
    if (!gid) return;
    if (!window.confirm(t("assistants.diarizationModelReinstallConfirm", { defaultValue: "Reinstall the speaker-label model?" }))) return;
    setDiarizationModelInstallBusy(true);
    setError("");
    setNotice("");
    try {
      const removeResp = await api.removeVoiceAssistantModel(gid, { modelId: DIARIZATION_MODEL_ID, by: "user" });
      if (!removeResp.ok) {
        setError(removeResp.error?.message || t("assistants.diarizationModelReinstallFailed", { defaultValue: "Failed to reinstall speaker-label model." }));
        return;
      }
      const installResp = await api.installVoiceAssistantModel(gid, { modelId: DIARIZATION_MODEL_ID, by: "user", background: true });
      if (!installResp.ok) {
        setError(installResp.error?.message || t("assistants.diarizationModelReinstallFailed", { defaultValue: "Failed to reinstall speaker-label model." }));
        return;
      }
      setNotice(t("assistants.diarizationModelReinstallStarted", { defaultValue: "Speaker-label model reinstall started." }));
      await loadAssistants({ quiet: true });
    } catch {
      setError(t("assistants.diarizationModelReinstallFailed", { defaultValue: "Failed to reinstall speaker-label model." }));
    } finally {
      setDiarizationModelInstallBusy(false);
    }
  };

  useEffect(() => {
    if (!isActive || !groupId || !showServiceModelControls || !selectedServiceModelInstalling) return undefined;
    const timer = window.setInterval(() => {
      void loadAssistants({ quiet: true });
    }, 1500);
    return () => window.clearInterval(timer);
  }, [groupId, isActive, loadAssistants, selectedServiceModelInstalling, showServiceModelControls]);

  if (!groupId) {
    return (
      <div className="space-y-4 animate-in fade-in slide-in-from-bottom-2 duration-300">
        <div>
          <h3 className="text-sm font-medium text-[var(--color-text-secondary)]">{t("assistants.title")}</h3>
          <p className="mt-1 text-xs text-[var(--color-text-muted)]">{t("assistants.openFromGroup")}</p>
        </div>
      </div>
    );
  }
  const parsedHelp = assistantHelpPrompt ? parseHelpMarkdown(String(assistantHelpPrompt.content || "")) : null;
  const savedPetPersona = parsedHelp?.pet || "";
  const savedVoiceSecretaryGuidance = parsedHelp?.voiceSecretary || "";
  const hasPetPersonaUnsaved = promptDraftDirty(
    savedPetPersona,
    petPersonaDraft,
    assistantHelpPrompt !== null,
    resolvePetPersonaDraft(""),
  );
  const hasVoiceGuidanceUnsaved = promptDraftDirty(
    savedVoiceSecretaryGuidance,
    voiceSecretaryGuidanceDraft,
    assistantHelpPrompt !== null,
    resolveVoiceSecretaryGuidanceDraft(""),
  );
  const showBrowserTranscriptBatching = recognitionBackend === "browser_asr";

  const renderVoiceGuidanceEditor = (expanded = false) => (
    <AssistantPromptEditor
      isDark={isDark}
      title={t("assistants.voiceGuidanceTitle")}
      hint={t("assistants.voiceGuidanceHint")}
      path={assistantHelpPrompt?.path || undefined}
      value={voiceSecretaryGuidanceDraft}
      placeholder={t("assistants.voiceGuidancePlaceholder")}
      busy={assistantPromptBusy}
      error={!assistantPromptFeedbackBlock || assistantPromptFeedbackBlock === "voice_secretary" ? assistantPromptError : ""}
      notice={assistantPromptFeedbackBlock === "voice_secretary" ? assistantPromptNotice : ""}
      hasUnsaved={hasVoiceGuidanceUnsaved}
      reloadLabel={assistantPromptBusy ? t("assistants.refreshing") : t("assistants.reloadAssistantGuidance")}
      discardLabel={t("assistants.discardAssistantGuidance")}
      saveLabel={assistantPromptBusy ? t("common:saving") : t("assistants.saveVoiceGuidance")}
      expandLabel={t("assistants.expandAssistantGuidance")}
      expanded={expanded}
      onReload={() => void loadAssistantGuidance({ force: true })}
      onDiscard={discardVoiceSecretaryGuidance}
      onSave={() => void saveAssistantGuidance("voice_secretary")}
      onExpand={() => setExpandedPromptBlock("voice_secretary")}
      onChange={(value) => {
        setVoiceSecretaryGuidanceDraft(value);
        setAssistantPromptNotice("");
        setAssistantPromptFeedbackBlock("");
      }}
    />
  );

  const renderPetPersonaEditor = (expanded = false) => (
    <AssistantPromptEditor
      isDark={isDark}
      title={t("assistants.petPersonaTitle")}
      hint={t("assistants.petPersonaHint")}
      path={assistantHelpPrompt?.path || undefined}
      value={petPersonaDraft}
      placeholder={t("assistants.petPersonaPlaceholder")}
      busy={assistantPromptBusy}
      error={!assistantPromptFeedbackBlock || assistantPromptFeedbackBlock === "pet" ? assistantPromptError : ""}
      notice={assistantPromptFeedbackBlock === "pet" ? assistantPromptNotice : ""}
      hasUnsaved={hasPetPersonaUnsaved}
      reloadLabel={assistantPromptBusy ? t("assistants.refreshing") : t("assistants.reloadAssistantGuidance")}
      discardLabel={t("assistants.discardAssistantGuidance")}
      saveLabel={assistantPromptBusy ? t("common:saving") : t("assistants.savePetPersona")}
      expandLabel={t("assistants.expandAssistantGuidance")}
      expanded={expanded}
      onReload={() => void loadAssistantGuidance({ force: true })}
      onDiscard={discardPetPersona}
      onSave={() => void saveAssistantGuidance("pet")}
      onExpand={() => setExpandedPromptBlock("pet")}
      onChange={(value) => {
        setPetPersonaDraft(value);
        setAssistantPromptNotice("");
        setAssistantPromptFeedbackBlock("");
      }}
    />
  );

  return (
    <div className="space-y-6 animate-in fade-in slide-in-from-bottom-2 duration-300">
      <AssistantsFeedbackToast error={error} notice={notice} />
      <div className={settingsWorkspaceShellClass(isDark)}>
        <div className={settingsWorkspaceHeaderClass(isDark)}>
          <div>
            <h3 className="text-sm font-semibold text-[var(--color-text-primary)]">{t("assistants.title")}</h3>
            <p className="mt-1 max-w-2xl text-xs leading-5 text-[var(--color-text-muted)]">
              {t("assistants.description")}
            </p>
          </div>
          <button
            type="button"
            onClick={() => {
              void loadAssistants();
              void loadAssistantGuidance({ force: true });
            }}
            disabled={loadBusy || assistantPromptBusy}
            className={secondaryButtonClass("sm")}
          >
            {loadBusy || assistantPromptBusy ? t("assistants.refreshing") : t("assistants.refresh")}
          </button>
        </div>

        <div className={settingsWorkspaceBodyClass}>
          <div className="space-y-5">
            <div className={settingsWorkspacePanelClass(isDark)}>
              <div className="flex flex-col gap-4 lg:flex-row lg:items-start lg:justify-between">
                <div className="min-w-0">
                  <div className="flex flex-wrap items-center gap-2">
                    <h4 className="text-sm font-semibold text-[var(--color-text-primary)]">{t("assistants.voiceTitle")}</h4>
                    <StatusPill tone={voiceEnabledTone}>{voiceEnabled ? t("assistants.enabled") : t("assistants.disabled")}</StatusPill>
                  </div>
                  <p className="mt-1 max-w-2xl text-xs leading-5 text-[var(--color-text-muted)]">
                    {t("assistants.voiceDescription")}
                  </p>
                </div>
                <AssistantSwitch
                  checked={voiceEnabled}
                  disabled={busy || voiceSaveBusy}
                  label={t("assistants.groupSwitch")}
                  onChange={(checked) => void toggleVoiceEnabled(checked)}
                />
              </div>

              <div className="mt-5 space-y-5">
                <SettingsBlock isDark={isDark} title={t("assistants.voiceRecognitionTitle")} hint={t("assistants.voiceRecognitionHint")}>
                  <div className="grid gap-4 md:grid-cols-1">
                    <div>
                      <label className={labelClass(isDark)}>{t("assistants.recognitionBackend")}</label>
                      <GroupCombobox
                        items={backendComboboxItems}
                        value={recognitionBackend}
                        onChange={setRecognitionBackend}
                        placeholder={t("assistants.recognitionBackend")}
                        searchPlaceholder={t("assistants.recognitionBackend")}
                        emptyText={t("common:noResults", { defaultValue: "No matching results" })}
                        ariaLabel={t("assistants.recognitionBackend")}
                        triggerClassName={`${inputClass(isDark)} min-h-[44px] cursor-pointer px-3 py-2 text-sm text-[var(--color-text-primary)]`}
                        contentClassName="p-0"
                        searchable={false}
                        matchTriggerWidth
                      />
                      {currentBackendUnavailable ? (
                        <p className="mt-1 text-[11px] leading-5 text-amber-700 dark:text-amber-300">
                          {t("assistants.recognitionBackendUnavailable")}
                        </p>
                      ) : null}
                      <p className="mt-1 text-[11px] leading-5 text-[var(--color-text-muted)]">
                        {t("assistants.recognitionBackendHint")}
                      </p>
                    </div>
                  </div>

                  {showServiceModelControls ? (
                    <div className={`mt-4 space-y-4 ${settingsWorkspaceSoftPanelClass(isDark)}`}>
                      <div className={localVoicePanelClass(isDark)}>
                        <div className="flex flex-wrap items-start justify-between gap-3">
                          <div className="min-w-0">
                            <div className="flex flex-wrap items-center gap-2">
                              <div className="text-sm font-semibold text-[var(--color-text-primary)]">
                                {t("assistants.localAsrTitle", { defaultValue: "Local ASR" })}
                              </div>
                              <StatusPill tone={localAsrStatusTone}>{localAsrStatusLabel}</StatusPill>
                              {localAsrDiskUsage ? <StatusPill tone="info">{localAsrDiskUsage}</StatusPill> : null}
                            </div>
                            <p className="mt-1 max-w-2xl text-[11px] leading-5 text-[var(--color-text-muted)]">
                              {t("assistants.localAsrHint", { defaultValue: "Install the local speech engine and the default streaming model for private live transcription on this device." })}
                            </p>
                          </div>
                          <button
                            type="button"
                            onClick={() => void (localAsrFailed ? reinstallLocalAsrBundle() : installLocalAsrBundle())}
                            disabled={busy || voiceSaveBusy || selectedServiceModelInstalling || !canManageLocalAsr || localAsrReady}
                            className={localAsrReady ? secondaryButtonClass("sm") : primaryButtonClass(false)}
                          >
                            {localAsrReady
                              ? t("assistants.localAsrInstalled", { defaultValue: "Installed" })
                              : localAsrFailed
                                ? t("assistants.localAsrRepair", { defaultValue: "Repair local ASR" })
                                : localAsrInstalling
                                  ? t("assistants.localAsrInstalling", { defaultValue: "Installing" })
                                  : t("assistants.localAsrInstall", { defaultValue: "Install local ASR" })}
                          </button>
                        </div>
                        <div className="mt-4 grid gap-2 lg:grid-cols-3">
                          <div className={localVoiceModelCardClass(isDark)}>
                            <div className="flex flex-wrap items-center gap-2">
                              <span className="text-[11px] font-semibold uppercase tracking-[0.14em] text-[var(--color-text-muted)]">
                                {t("assistants.localAsrEngineLabel", { defaultValue: "Engine" })}
                              </span>
                              <StatusPill tone={streamingRuntimeReady ? "on" : streamingRuntimeStatus === "failed" ? "off" : "info"}>
                                {t("assistants.componentStatusShort", { status: streamingRuntimeStatus, defaultValue: "{{status}}" })}
                              </StatusPill>
                            </div>
                            <p className="mt-1 text-[11px] leading-5 text-[var(--color-text-muted)]">
                              {t("assistants.localAsrEngineHint", { defaultValue: "sherpa-onnx runtime environment." })}
                            </p>
                          </div>
                          <div className={localVoiceModelCardClass(isDark)}>
                            <div className="flex flex-wrap items-center gap-2">
                              <span className="text-[11px] font-semibold uppercase tracking-[0.14em] text-[var(--color-text-muted)]">
                                {t("assistants.liveAsrModelLabel", { defaultValue: "Live ASR" })}
                              </span>
                              <StatusPill tone={liveServiceAsrModelReady ? "on" : liveServiceAsrModelStatus === "failed" ? "off" : "info"}>
                                {liveServiceAsrModelStatus === "downloading"
                                  ? `${t("assistants.componentStatusShort", { status: liveServiceAsrModelStatus, defaultValue: "{{status}}" })} ${Math.round(Number(liveServiceAsrModel?.progress_percent || 0))}%`
                                  : t("assistants.componentStatusShort", { status: liveServiceAsrModelStatus, defaultValue: "{{status}}" })}
                              </StatusPill>
                              {liveServiceAsrModelSize ? <StatusPill tone="info">{liveServiceAsrModelSize}</StatusPill> : null}
                            </div>
                            <p className="mt-1 break-words text-[11px] leading-5 text-[var(--color-text-muted)]">
                              {liveServiceAsrModel?.title || liveServiceAsrModelId || t("assistants.streamingAsrModelMissing", { defaultValue: "No streaming ASR model is available." })}
                            </p>
                          </div>
                          <div className={localVoiceModelCardClass(isDark)}>
                            <div className="flex flex-wrap items-center gap-2">
                              <span className="text-[11px] font-semibold uppercase tracking-[0.14em] text-[var(--color-text-muted)]">
                                {t("assistants.finalAsrModelLabel", { defaultValue: "Final ASR" })}
                              </span>
                              <StatusPill tone={finalServiceAsrModelReady ? "on" : finalServiceAsrModelStatus === "failed" ? "off" : "info"}>
                                {finalServiceAsrModelStatus === "downloading"
                                  ? `${t("assistants.componentStatusShort", { status: finalServiceAsrModelStatus, defaultValue: "{{status}}" })} ${Math.round(Number(finalServiceAsrModel?.progress_percent || 0))}%`
                                  : t("assistants.componentStatusShort", { status: finalServiceAsrModelStatus, defaultValue: "{{status}}" })}
                              </StatusPill>
                              {finalServiceAsrModelSize ? <StatusPill tone="info">{finalServiceAsrModelSize}</StatusPill> : null}
                            </div>
                            <p className="mt-1 break-words text-[11px] leading-5 text-[var(--color-text-muted)]">
                              {finalServiceAsrModel?.title || finalServiceAsrModelId || t("assistants.finalAsrModelMissing", { defaultValue: "No final ASR model is available." })}
                            </p>
                          </div>
                        </div>
                        {streamingRuntime?.error?.message || liveServiceAsrModel?.error?.message || finalServiceAsrModel?.error?.message ? (
                          <p className="mt-3 text-[11px] leading-5 text-rose-700 dark:text-rose-300">
                            {t("assistants.serviceRuntimeError", { message: String(streamingRuntime?.error?.message || liveServiceAsrModel?.error?.message || finalServiceAsrModel?.error?.message || "") })}
                          </p>
                        ) : null}
                      </div>

                      <div className={`flex flex-wrap items-start justify-between gap-3 ${localVoicePanelClass(isDark)}`}>
                        <div className="min-w-0">
                          <div className="flex flex-wrap items-center gap-2">
                            <div className="text-sm font-semibold text-[var(--color-text-primary)]">
                              {t("assistants.speakerLabelsTitle", { defaultValue: "Speaker labels" })}
                            </div>
                            <StatusPill tone={diarizationModelReady ? "on" : diarizationModelStatus === "failed" ? "off" : "info"}>
                              {diarizationModelStatus === "downloading"
                                ? `${t("assistants.componentStatusShort", { status: diarizationModelStatus, defaultValue: "{{status}}" })} ${Math.round(Number(diarizationModel?.progress_percent || 0))}%`
                                : t("assistants.componentStatusShort", { status: diarizationModelStatus, defaultValue: "{{status}}" })}
                            </StatusPill>
                            <StatusPill tone="info">{t("assistants.optional", { defaultValue: "Optional" })}</StatusPill>
                            {diarizationModelSize ? <StatusPill tone="info">{diarizationModelSize}</StatusPill> : null}
                          </div>
                          <p className="mt-1 text-[11px] leading-5 text-[var(--color-text-muted)]">
                            {t("assistants.speakerLabelsHint", { defaultValue: "Adds anonymous Speaker 1 / Speaker 2 turns after local ASR recordings. Local transcription works without this model." })}
                          </p>
                          {diarizationModel?.error?.message ? (
                            <p className="mt-1 text-[11px] leading-5 text-rose-700 dark:text-rose-300">
                              {t("assistants.serviceRuntimeError", { message: String(diarizationModel.error.message || "") })}
                            </p>
                          ) : null}
                        </div>
                        <button
                          type="button"
                          onClick={() => void installDiarizationModel()}
                          disabled={busy || voiceSaveBusy || diarizationModelInstalling || diarizationModelReady}
                          className={secondaryButtonClass("sm")}
                        >
                          {diarizationModelReady
                            ? t("assistants.diarizationModelInstalled", { defaultValue: "Model installed" })
                            : diarizationModelInstalling
                              ? t("assistants.diarizationModelInstalling", { defaultValue: "Downloading..." })
                              : t("assistants.diarizationModelInstall", { defaultValue: "Install speaker model" })}
                        </button>
                      </div>

                      <details>
                        <summary className="cursor-pointer text-xs font-semibold text-[var(--color-text-secondary)]">
                          {t("assistants.localAsrMaintenanceTitle", { defaultValue: "Advanced maintenance" })}
                        </summary>
                        <div className="mt-3 rounded-xl border border-black/5 bg-white/35 p-3 dark:border-white/10 dark:bg-white/[0.04]">
                          <div className="grid gap-2 text-[11px] leading-5 text-[var(--color-text-muted)] md:grid-cols-2">
                            <div>
                              <span className="font-semibold text-[var(--color-text-secondary)]">{t("assistants.localAsrCacheLabel", { defaultValue: "Local ASR cache" })}: </span>
                              {localAsrDiskUsage || t("assistants.none", { defaultValue: "none" })}
                            </div>
                            <div>
                              <span className="font-semibold text-[var(--color-text-secondary)]">{t("assistants.speakerLabelsCacheLabel", { defaultValue: "Speaker-label cache" })}: </span>
                              {diarizationModelDiskSize || t("assistants.none", { defaultValue: "none" })}
                            </div>
                            <div className="break-words md:col-span-2">
                              <span className="font-semibold text-[var(--color-text-secondary)]">{t("assistants.localAsrRuntimePath", { defaultValue: "Runtime path" })}: </span>
                              {streamingRuntime?.install_dir || "-"}
                            </div>
                            <div className="break-words md:col-span-2">
                              <span className="font-semibold text-[var(--color-text-secondary)]">{t("assistants.liveAsrModelPath", { defaultValue: "Live model path" })}: </span>
                              {liveServiceAsrModel?.install_dir || "-"}
                            </div>
                            <div className="break-words md:col-span-2">
                              <span className="font-semibold text-[var(--color-text-secondary)]">{t("assistants.finalAsrModelPath", { defaultValue: "Final model path" })}: </span>
                              {finalServiceAsrModel?.install_dir || "-"}
                            </div>
                          </div>
                          <div className="mt-3 flex flex-wrap gap-2">
                            <button
                              type="button"
                              onClick={() => void reinstallLocalAsrBundle()}
                              disabled={busy || voiceSaveBusy || selectedServiceModelInstalling || !canManageLocalAsr}
                              className={secondaryButtonClass("sm")}
                            >
                              {t("assistants.localAsrReinstall", { defaultValue: "Reinstall local ASR" })}
                            </button>
                            <button
                              type="button"
                              onClick={() => void removeLocalAsrBundle()}
                              disabled={busy || voiceSaveBusy || selectedServiceModelInstalling || !canManageLocalAsr}
                              className={secondaryButtonClass("sm")}
                            >
                              {t("assistants.localAsrRemove", { defaultValue: "Remove local ASR" })}
                            </button>
                            <button
                              type="button"
                              onClick={() => void reinstallDiarizationModel()}
                              disabled={busy || voiceSaveBusy || selectedServiceModelInstalling}
                              className={secondaryButtonClass("sm")}
                            >
                              {t("assistants.diarizationModelReinstall", { defaultValue: "Reinstall speaker labels" })}
                            </button>
                            <button
                              type="button"
                              onClick={() => void removeDiarizationModel()}
                              disabled={busy || voiceSaveBusy || selectedServiceModelInstalling || !diarizationModel?.model_id}
                              className={secondaryButtonClass("sm")}
                            >
                              {t("assistants.diarizationModelRemove", { defaultValue: "Remove speaker labels" })}
                            </button>
                          </div>
                        </div>
                      </details>
                    </div>
                  ) : null}

                  {showBrowserTranscriptBatching ? (
                    <div className={`mt-4 ${settingsWorkspaceSoftPanelClass(isDark)}`}>
                      <div className="flex flex-wrap items-start justify-between gap-3">
                        <div className="min-w-0">
                          <div className="text-xs font-semibold text-[var(--color-text-primary)]">
                            {t("assistants.transcriptBatchingTitle")}
                          </div>
                          <p className="mt-1 text-[11px] leading-5 text-[var(--color-text-muted)]">
                            {t("assistants.transcriptBatchingHint")}
                          </p>
                        </div>
                        <button
                          type="button"
                          onClick={() => void resetVoiceBatching()}
                          disabled={busy || voiceSaveBusy}
                          className={secondaryButtonClass("sm")}
                        >
                          {t("assistants.resetTranscriptBatching")}
                        </button>
                      </div>
                      <div className="mt-3 grid gap-4 md:grid-cols-2">
                        <div>
                          <label className={labelClass(isDark)}>{t("assistants.transcriptQuietWindow")}</label>
                          <div className="flex items-center gap-2">
                            <input
                              type="number"
                              min={VOICE_MIN_QUIET_SECONDS}
                              max={VOICE_MAX_QUIET_SECONDS}
                              step={1}
                              value={voiceQuietWindowSeconds}
                              onChange={(event) => {
                                const value = Number(event.target.value);
                                if (Number.isFinite(value)) setVoiceQuietWindowSeconds(value);
                              }}
                              className={inputClass(isDark)}
                            />
                            <span className="shrink-0 text-xs text-[var(--color-text-muted)]">
                              {t("assistants.secondsUnit")}
                            </span>
                          </div>
                          <p className="mt-1 text-[11px] leading-5 text-[var(--color-text-muted)]">
                            {t("assistants.transcriptQuietWindowHint")}
                          </p>
                        </div>
                        <div>
                          <label className={labelClass(isDark)}>{t("assistants.transcriptMaxWindow")}</label>
                          <div className="flex items-center gap-2">
                            <input
                              type="number"
                              min={VOICE_MIN_MAX_WINDOW_SECONDS}
                              max={VOICE_MAX_MAX_WINDOW_SECONDS}
                              step={1}
                              value={voiceMaxWindowSeconds}
                              onChange={(event) => {
                                const value = Number(event.target.value);
                                if (Number.isFinite(value)) setVoiceMaxWindowSeconds(value);
                              }}
                              className={inputClass(isDark)}
                            />
                            <span className="shrink-0 text-xs text-[var(--color-text-muted)]">
                              {t("assistants.secondsUnit")}
                            </span>
                          </div>
                          <p className="mt-1 text-[11px] leading-5 text-[var(--color-text-muted)]">
                            {t("assistants.transcriptMaxWindowHint")}
                          </p>
                        </div>
                      </div>
                    </div>
                  ) : null}

                  {showServiceAsrDiagnostic ? (
                    <div className="mt-4 rounded-lg border border-[var(--glass-border-subtle)] bg-[var(--color-bg-secondary)] px-3 py-2 text-xs leading-5 text-[var(--color-text-muted)]">
                      <div className="flex flex-wrap items-center justify-between gap-2">
                        <div className="font-medium text-[var(--color-text-secondary)]">{t("assistants.serviceAsrStatus")}</div>
                        <StatusPill tone={serviceTone}>
                          {t("assistants.serviceAsrStatusValue", { status: serviceStatus || "not_started" })}
                        </StatusPill>
                      </div>
                      <div className="mt-1">
                        {asrCommandConfigured
                          ? (
                            serviceHealth.managed_asr_command_configured
                              ? t("assistants.serviceAsrManagedModelReady")
                              : t("assistants.serviceAsrConfigured")
                          )
                          : streamingRuntimeReady
                            ? t("assistants.streamingRuntimeNotConnected")
                            : t("assistants.serviceAsrMissingCommand")}
                      </div>
                      {serviceLastErrorMessage ? (
                        <div className="mt-1 text-rose-700 dark:text-rose-300">
                          {t("assistants.serviceAsrLastError", { message: serviceLastErrorMessage })}
                        </div>
                      ) : null}
                    </div>
                  ) : null}

                  <div className="mt-4 flex justify-end">
                    <button
                      type="button"
                      onClick={() => void saveVoiceSettings()}
                      disabled={busy || voiceSaveBusy}
                      className={primaryButtonClass(voiceSaveBusy)}
                    >
                      {voiceSaveBusy ? t("common:saving") : t("assistants.saveVoiceRecognition")}
                    </button>
                  </div>
                </SettingsBlock>

                {renderVoiceGuidanceEditor()}
              </div>
            </div>

            <div className={settingsWorkspacePanelClass(isDark)}>
              <div className="flex flex-col gap-4 lg:flex-row lg:items-start lg:justify-between">
                <div className="min-w-0">
                  <div className="flex flex-wrap items-center gap-2">
                    <h4 className="text-sm font-semibold text-[var(--color-text-primary)]">{t("assistants.petTitle")}</h4>
                    <StatusPill tone={effectivePetEnabled ? "on" : "off"}>
                      {effectivePetEnabled ? t("assistants.enabled") : t("assistants.disabled")}
                    </StatusPill>
                  </div>
                  <p className="mt-1 max-w-2xl text-xs leading-5 text-[var(--color-text-muted)]">
                    {t("assistants.petDescription")}
                  </p>
                </div>
                <AssistantSwitch
                  checked={effectivePetEnabled}
                  disabled={busy || petSaveBusy || !onUpdatePetEnabled}
                  label={t("assistants.groupSwitch")}
                  onChange={(checked) => void togglePet(checked)}
                />
              </div>

              <div className="mt-5">
                {renderPetPersonaEditor()}
              </div>
            </div>
          </div>
        </div>
      </div>

      {expandedPromptBlock
        ? (
          <BodyPortal>
            <div
              key={expandedPromptBlock}
              className="fixed inset-0 z-[1000] animate-fade-in"
              role="dialog"
              aria-modal="true"
              onPointerDown={(event) => {
                if (event.target === event.currentTarget) setExpandedPromptBlock(null);
              }}
            >
              <div className="absolute inset-0 glass-overlay" />
              <div className={settingsDialogPanelClass("xl")}>
                <div className="flex shrink-0 justify-end border-b border-[var(--glass-border-subtle)] px-3 py-2 sm:px-4 sm:py-3">
                  <button type="button" className={secondaryButtonClass("sm")} onClick={() => setExpandedPromptBlock(null)}>
                    {t("common:close")}
                  </button>
                </div>
                <div className={settingsDialogBodyClass}>
                  {expandedPromptBlock === "voice_secretary" ? renderVoiceGuidanceEditor(true) : renderPetPersonaEditor(true)}
                </div>
              </div>
            </div>
          </BodyPortal>
          )
        : null}
    </div>
  );
}
