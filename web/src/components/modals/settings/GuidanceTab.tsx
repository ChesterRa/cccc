import { useEffect, useMemo, useState } from "react";
import { Trans, useTranslation } from "react-i18next";
import * as api from "../../../services/api";
import type { Actor } from "../../../types";
import { buildHelpMarkdown, parseHelpMarkdown, type HelpChangedBlock, type ParsedHelpMarkdown } from "../../../utils/helpMarkdown";
import { BodyPortal } from "../../ui/BodyPortal";
import {
  cardClass,
  inputClass,
  labelClass,
  primaryButtonClass,
  secondaryButtonClass,
  settingsDialogBodyClass,
  settingsDialogPanelClass,
} from "./types";

type PromptKind = "preamble" | "help";
type PromptInfo = api.GroupPromptInfo;
type HelpViewMode = "structured" | "raw";
type HelpScopeId = "common" | "role:foreman" | "role:peer" | `actor:${string}`;

const EMPTY_HELP: ParsedHelpMarkdown = {
  common: "",
  foreman: "",
  peer: "",
  voiceSecretary: "",
  actorNotes: {},
  extraTaggedBlocks: [],
};

function displayActorName(actor: Actor): string {
  return String(actor.title || actor.id || "").trim() || String(actor.id || "").trim();
}

function uniqueChangedBlocks(blocks: HelpChangedBlock[]): HelpChangedBlock[] {
  const seen = new Set<string>();
  const out: HelpChangedBlock[] = [];
  for (const block of blocks) {
    const key = String(block || "").trim();
    if (!key || seen.has(key)) continue;
    seen.add(key);
    out.push(block);
  }
  return out;
}

export function GuidanceTab({ isDark, groupId }: {
  isDark: boolean;
  groupId?: string;
}) {
  const { t } = useTranslation("settings");
  const [busy, setBusy] = useState(false);
  const [err, setErr] = useState("");
  const [prompts, setPrompts] = useState<Record<PromptKind, PromptInfo> | null>(null);
  const [actors, setActors] = useState<Actor[]>([]);
  const [expandedKind, setExpandedKind] = useState<PromptKind | null>(null);
  const [helpViewMode, setHelpViewMode] = useState<HelpViewMode>("structured");
  const [helpStructured, setHelpStructured] = useState<ParsedHelpMarkdown>(EMPTY_HELP);
  const [helpTouchedRaw, setHelpTouchedRaw] = useState(false);
  const [helpChangedBlocks, setHelpChangedBlocks] = useState<HelpChangedBlock[]>([]);
  const [selectedHelpScope, setSelectedHelpScope] = useState<HelpScopeId>("common");

  const actorIds = useMemo(
    () => actors.map((actor) => String(actor.id || "").trim()).filter(Boolean),
    [actors]
  );
  const actorIdSet = useMemo(() => new Set(actorIds), [actorIds]);
  const orphanActorIds = useMemo(
    () => Object.keys(helpStructured.actorNotes).filter((actorId) => !actorIdSet.has(actorId)).sort(),
    [helpStructured.actorNotes, actorIdSet]
  );

  const syncHelpState = (content: string) => {
    setHelpStructured(parseHelpMarkdown(content));
  };

  const load = async () => {
    if (!groupId) return;
    setBusy(true);
    setErr("");
    try {
      const [promptsResp, actorsResp] = await Promise.all([
        api.fetchGroupPrompts(groupId),
        api.fetchActors(groupId, false),
      ]);
      if (!promptsResp.ok) {
        setErr(promptsResp.error?.message || t("guidance.failedToLoad"));
        setPrompts(null);
        setActors([]);
        return;
      }
      const p = promptsResp.result?.preamble;
      const h = promptsResp.result?.help;
      if (!p || !h) {
        setErr(t("guidance.invalidResponse"));
        setPrompts(null);
        setActors([]);
        return;
      }
      const nextActors = actorsResp.ok ? (actorsResp.result?.actors || []) : [];
      setPrompts({ preamble: p, help: h });
      setActors(nextActors);
      syncHelpState(String(h.content || ""));
      setHelpTouchedRaw(false);
      setHelpChangedBlocks([]);
      setHelpViewMode("structured");
      setSelectedHelpScope("common");
    } catch {
      setErr(t("guidance.failedToLoad"));
      setPrompts(null);
      setActors([]);
    } finally {
      setBusy(false);
    }
  };

  useEffect(() => {
    if (groupId) void load();
    // eslint-disable-next-line react-hooks/exhaustive-deps -- reload when group changes.
  }, [groupId]);

  const setPromptContent = (kind: PromptKind, content: string) => {
    setPrompts((current) => {
      if (!current) return current;
      return { ...current, [kind]: { ...current[kind], content } };
    });
  };

  const setHelpContentRaw = (content: string) => {
    setPromptContent("help", content);
    setHelpStructured(parseHelpMarkdown(content));
    setHelpTouchedRaw(true);
    setHelpChangedBlocks([]);
  };

  const applyStructuredHelp = (next: ParsedHelpMarkdown, changed: HelpChangedBlock) => {
    const content = buildHelpMarkdown({
      common: next.common,
      foreman: next.foreman,
      peer: next.peer,
      voiceSecretary: next.voiceSecretary,
      actorNotes: next.actorNotes,
      actorOrder: actorIds,
      extraTaggedBlocks: next.extraTaggedBlocks,
    });
    setPromptContent("help", content);
    // Keep the live textarea state as typed. A build->parse roundtrip trims
    // trailing blank lines, which makes Enter feel broken in structured mode.
    setHelpStructured(next);
    setHelpChangedBlocks((current) => uniqueChangedBlocks([...current, changed]));
  };

  const updateCommon = (value: string) => {
    applyStructuredHelp({ ...helpStructured, common: value }, "common");
  };

  const updateRole = (role: "foreman" | "peer", value: string) => {
    if (role === "foreman") {
      applyStructuredHelp({ ...helpStructured, foreman: value }, "role:foreman");
      return;
    }
    applyStructuredHelp({ ...helpStructured, peer: value }, "role:peer");
  };

  const updateActorNote = (actorId: string, value: string) => {
    const nextActorNotes = { ...helpStructured.actorNotes, [actorId]: value };
    if (!String(value || "").trim()) delete nextActorNotes[actorId];
    applyStructuredHelp({ ...helpStructured, actorNotes: nextActorNotes }, `actor:${actorId}`);
  };

  const savePrompt = async (kind: PromptKind) => {
    if (!groupId || !prompts) return;
    setBusy(true);
    setErr("");
    try {
      const resp = await api.updateGroupPrompt(groupId, kind, prompts[kind].content || "");
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

  const saveHelp = async () => {
    if (!groupId || !prompts) return;
    setBusy(true);
    setErr("");
    try {
      const resp = await api.updateGroupPrompt(
        groupId,
        "help",
        prompts.help.content || "",
        helpTouchedRaw
          ? { editorMode: "raw" }
          : { editorMode: "structured", changedBlocks: helpChangedBlocks }
      );
      if (!resp.ok) {
        setErr(resp.error?.message || t("guidance.failedToSave", { kind: "help" }));
        return;
      }
      await load();
    } catch {
      setErr(t("guidance.failedToSave", { kind: "help" }));
    } finally {
      setBusy(false);
    }
  };

  const resetPrompt = async (kind: PromptKind) => {
    if (!groupId || !prompts) return;
    const filename = prompts[kind]?.filename || kind;
    const ok = window.confirm(t("automation.resetGuidanceConfirm", { kind, filename }));
    if (!ok) return;
    setBusy(true);
    setErr("");
    try {
      const resp = await api.resetGroupPrompt(groupId, kind);
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

  if (!groupId) {
    return (
      <div className={cardClass(isDark)}>
        <div className="text-sm text-[var(--color-text-secondary)]">{t("guidance.openFromGroup")}</div>
      </div>
    );
  }

  const preamble = prompts?.preamble;
  const help = prompts?.help;
  const helpSource = help?.source || "builtin";
  const helpBadge =
    helpSource === "home"
      ? isDark
        ? "bg-white/[0.07] text-white border border-white/10"
        : "bg-[rgb(245,245,245)] text-[rgb(35,36,37)] border border-black/10"
      : isDark
        ? "bg-white/[0.04] text-white/68 border border-white/8"
        : "bg-[var(--glass-tab-bg)] text-[var(--color-text-secondary)] border border-[var(--glass-border-subtle)]";

  const preambleSource = preamble?.source || "builtin";
  const preambleBadge =
    preambleSource === "home"
      ? isDark
        ? "bg-white/[0.07] text-white border border-white/10"
        : "bg-[rgb(245,245,245)] text-[rgb(35,36,37)] border border-black/10"
      : isDark
        ? "bg-white/[0.04] text-white/68 border border-white/8"
        : "bg-[var(--glass-tab-bg)] text-[var(--color-text-secondary)] border border-[var(--glass-border-subtle)]";
  const settingsScrollAreaClass = "overflow-y-auto scrollbar-subtle pr-2 pb-2 [scrollbar-gutter:stable]";
  const promptShellClass = `overflow-hidden rounded-[22px] border backdrop-blur-xl ${
    isDark
      ? "border-white/10 bg-[linear-gradient(180deg,rgba(19,20,24,0.88),rgba(10,11,14,0.96))] shadow-[0_28px_100px_rgba(0,0,0,0.36)]"
      : "border-black/8 bg-[linear-gradient(180deg,rgba(255,255,255,0.995),rgba(246,248,251,0.96))] shadow-[0_28px_100px_rgba(15,23,42,0.06)]"
  }`;
  const promptHeaderClass = `flex items-start justify-between gap-4 px-4 py-4 sm:px-5 sm:py-4 ${
    isDark ? "border-b border-white/8 bg-white/[0.025]" : "border-b border-black/6 bg-[rgba(18,18,20,0.018)]"
  }`;
  const promptHeaderTextClass = isDark ? "text-white" : "text-[rgb(22,24,29)]";
  const promptHintClass = "text-[var(--color-text-tertiary)]";
  const promptBodyClass = (expanded = false) => `px-4 py-4 sm:px-5 sm:py-5 ${expanded ? "min-h-0 flex flex-1 flex-col" : "space-y-4"}`;
  const promptPathClass = `inline-flex max-w-full items-center rounded-full border px-3 py-1 text-[11px] font-mono leading-5 ${
    isDark
      ? "border-white/8 bg-white/[0.03] text-white/64"
      : "border-[var(--glass-border-subtle)] bg-[var(--glass-tab-bg)] text-[var(--color-text-secondary)]"
  }`;
  const editorSurfaceSoftClass = `rounded-[18px] border px-4 py-3 sm:px-4 sm:py-4 ${
    isDark
      ? "border-white/8 bg-white/[0.025]"
      : "border-black/6 bg-[linear-gradient(180deg,rgba(255,255,255,0.94),rgba(246,248,251,0.88))]"
  }`;
  const editorTextareaClass = `${inputClass(isDark)} min-h-[320px] resize-y border-0 bg-transparent px-0 py-0 shadow-none focus-visible:ring-0`;
  const editorMetaBadgeClass = `inline-flex items-center rounded-full px-2.5 py-1 text-[10px] font-medium border border-[var(--glass-border-subtle)] bg-[var(--glass-tab-bg)] text-[var(--color-text-secondary)]`;
  const segmentedControlClass = `inline-flex rounded-full border p-1 ${
    isDark ? "border-white/8 bg-white/[0.025]" : "border-[var(--glass-border-subtle)] bg-[var(--glass-tab-bg)]"
  }`;
  const navigationPanelClass = `rounded-[18px] border p-3 ${
    isDark
      ? "border-white/10 bg-[linear-gradient(180deg,rgba(24,26,31,0.86),rgba(14,15,19,0.96))]"
      : "border-black/8 bg-[linear-gradient(180deg,rgba(255,255,255,0.96),rgba(246,248,251,0.92))]"
  }`;
  const workspacePanelClass = `rounded-[18px] border p-3.5 sm:p-4 ${
    isDark
      ? "border-white/10 bg-[linear-gradient(180deg,rgba(24,26,31,0.9),rgba(13,14,18,0.98))]"
      : "border-black/8 bg-[linear-gradient(180deg,rgba(255,255,255,0.995),rgba(246,248,251,0.96))]"
  }`;
  const navSectionTitleClass = `mb-2 text-[10px] font-semibold uppercase tracking-[0.18em] text-[var(--color-text-muted)]`;
  const overridesHintClass = `rounded-[18px] border px-4 py-3 text-[11px] leading-5 ${
    isDark
      ? "border-white/10 bg-[linear-gradient(180deg,rgba(255,255,255,0.04),rgba(255,255,255,0.02))] text-white/50"
      : "border-black/8 bg-[linear-gradient(180deg,rgba(255,255,255,0.98),rgba(245,248,252,0.9))] text-[rgb(91,92,97)]"
  }`;

  const renderSourceBadge = (kind: PromptKind) => {
    const badgeClass = kind === "help" ? helpBadge : preambleBadge;
    const source = kind === "help" ? helpSource : preambleSource;
    return (
      <div className={`inline-flex items-center rounded-full px-3 py-1.5 text-[11px] font-medium ${badgeClass}`}>
        {source === "home" ? t("guidance.overrideBadge") : t("guidance.builtinBadge")}
      </div>
    );
  };

  const renderPromptActions = (kind: PromptKind, expanded = false) => {
    const source = kind === "help" ? helpSource : preambleSource;
    const handleSave = kind === "help" ? () => void saveHelp() : () => void savePrompt("preamble");
    return (
      <div
        className={
          expanded
            ? "mt-4 flex flex-wrap items-center gap-2"
            : `mt-0 flex flex-wrap items-center gap-2 border-t px-4 py-3 sm:px-5 ${
                isDark ? "border-white/8 bg-white/[0.02]" : "border-black/6 bg-black/[0.015]"
              }`
        }
      >
        <button className={primaryButtonClass(busy)} onClick={handleSave} disabled={busy}>
          {t("common:save")}
        </button>
        <button
          type="button"
          className={secondaryButtonClass()}
          onClick={() => void resetPrompt(kind)}
          disabled={busy || source !== "home"}
          title={source === "home" ? t("guidance.resetHint") : t("guidance.noOverride")}
        >
          {t("common:reset")}
        </button>
        <button
          type="button"
          className={`${secondaryButtonClass()} ml-auto`}
          onClick={() => void load()}
          disabled={busy}
          title={t("guidance.discardChanges")}
        >
          {t("guidance.discardChanges")}
        </button>
      </div>
    );
  };

  const renderPreambleCard = (expanded = false) => (
    <div className={expanded ? "flex h-full min-h-0 flex-col" : promptShellClass}>
      <div className={expanded ? "flex items-start justify-between gap-3" : promptHeaderClass}>
        <div className="min-w-0">
          <div className={`text-sm font-semibold ${promptHeaderTextClass}`}>{t("guidance.preambleTitle")}</div>
          <div className={`text-[11px] ${promptHintClass}`}>{t("guidance.preambleHint")}</div>
        </div>
        <div className="flex items-center gap-2 shrink-0">
          {!expanded ? (
            <button
              type="button"
              className={secondaryButtonClass("sm")}
              onClick={() => setExpandedKind("preamble")}
              disabled={busy}
              title={t("guidance.expandTitle")}
            >
              {t("guidance.expand")}
            </button>
          ) : null}
          {renderSourceBadge("preamble")}
        </div>
      </div>

      <div className={expanded ? "mt-3 min-h-0 flex flex-1 flex-col" : promptBodyClass(expanded)}>
        {preamble?.path ? (
          <div className={promptPathClass}>
            <span className="truncate">{preamble.path}</span>
          </div>
        ) : null}

        <div className={`${editorSurfaceSoftClass} ${expanded ? "min-h-0 flex flex-1 flex-col" : ""}`}>
          <div className="mb-4 flex items-center justify-between gap-3">
            <label className={labelClass(isDark)}>{t("guidance.markdown")}</label>
            <div className={editorMetaBadgeClass}>Markdown</div>
          </div>
          <textarea
            className={`${editorTextareaClass} font-mono text-[12px] ${expanded ? "min-h-[440px] flex-1" : ""}`}
            style={expanded ? undefined : { minHeight: 220 }}
            value={preamble?.content || ""}
            onChange={(e) => setPromptContent("preamble", e.target.value)}
            spellCheck={false}
          />
        </div>
      </div>

      {renderPromptActions("preamble", expanded)}
    </div>
  );

  const commonScope = {
    id: "common" as HelpScopeId,
    title: t("guidance.commonNotesTitle", "Common Notes"),
    hint: t("guidance.commonNotesHint", "Untagged help content shared by all actors."),
    placeholder: t("guidance.commonNotesPlaceholder", "Keep shared guidance, workflow details, and appendices here..."),
    value: helpStructured.common,
    roleLabel: undefined as string | undefined,
    isOrphan: false,
  };

  const foremanScope = {
    id: "role:foreman" as HelpScopeId,
    title: t("guidance.foremanNotesTitle", "Foreman Notes"),
    hint: t("guidance.foremanNotesHint", "Only foreman actors receive this scoped block."),
    placeholder: t("guidance.foremanNotesPlaceholder", "Own outcome quality, review peer outputs, and keep shared direction coherent..."),
    value: helpStructured.foreman,
    roleLabel: undefined as string | undefined,
    isOrphan: false,
  };

  const peerScope = {
    id: "role:peer" as HelpScopeId,
    title: t("guidance.peerNotesTitle", "Peer Notes"),
    hint: t("guidance.peerNotesHint", "Only peer actors receive this scoped block."),
    placeholder: t("guidance.peerNotesPlaceholder", "Report risks early, deliver verifiable outputs, and say when the direction is wrong..."),
    value: helpStructured.peer,
    roleLabel: undefined as string | undefined,
    isOrphan: false,
  };

  const actorScopes = actors.map((actor) => {
    const actorId = String(actor.id || "").trim();
    const note = String(helpStructured.actorNotes[actorId] || "");
    const roleLabel = String(actor.role || t("guidance.unknownRole", "Unknown")).trim() || t("guidance.unknownRole", "Unknown");
    return {
      id: `actor:${actorId}` as HelpScopeId,
      title: displayActorName(actor),
      hint: t("guidance.actorNotesHint", "Local notes for specific actors. This is the same source edited from the actor modal shortcut."),
      placeholder: t("guidance.actorNotePlaceholder", "Describe only this actor's local responsibilities, boundaries, and preferred behavior..."),
      value: note,
      roleLabel,
      isOrphan: false,
    };
  });

  const orphanActorScopes = orphanActorIds.map((actorId) => {
    const note = String(helpStructured.actorNotes[actorId] || "");
    return {
      id: `actor:${actorId}` as HelpScopeId,
      title: actorId,
      hint: t("guidance.actorNotesHint", "Local notes for specific actors. This is the same source edited from the actor modal shortcut."),
      placeholder: t("guidance.orphanActorNotePlaceholder", "Keep or clean this leftover note for an actor that no longer exists..."),
      value: note,
      roleLabel: t("guidance.orphanActorRole", "No longer in group"),
      isOrphan: true,
    };
  });

  const selectedHelpScopeItem = [commonScope, foremanScope, peerScope, ...actorScopes, ...orphanActorScopes].find(
    (item) => item.id === selectedHelpScope
  ) || commonScope;

  const updateSelectedHelpScopeValue = (value: string) => {
    if (selectedHelpScopeItem.id === "common") {
      updateCommon(value);
      return;
    }
    if (selectedHelpScopeItem.id === "role:foreman") {
      updateRole("foreman", value);
      return;
    }
    if (selectedHelpScopeItem.id === "role:peer") {
      updateRole("peer", value);
      return;
    }
    updateActorNote(selectedHelpScopeItem.id.slice("actor:".length), value);
  };

  const renderHelpScopeButton = (item: {
    id: HelpScopeId;
    title: string;
    roleLabel?: string;
  }) => {
    const active = item.id === selectedHelpScope;
    return (
      <button
        key={item.id}
        type="button"
        className={`w-full text-left rounded-2xl border px-3 py-2.5 transition-colors ${
          active
            ? isDark
              ? "border-white/10 bg-white/[0.08] text-white shadow-[inset_0_1px_0_rgba(255,255,255,0.03)]"
              : "border-black/10 bg-white text-[rgb(35,36,37)] shadow-[0_8px_30px_rgba(15,23,42,0.08)]"
            : isDark
              ? "border-white/6 bg-white/[0.018] text-white/70 hover:bg-white/[0.04]"
              : "border-black/6 bg-black/[0.02] text-gray-800 hover:bg-black/[0.03]"
        }`}
        onClick={() => setSelectedHelpScope(item.id)}
      >
        <div className="flex items-center gap-1.5 min-w-0">
          <span className="font-medium truncate">{item.title}</span>
          {item.roleLabel ? (
            <span className={`text-[10px] px-1.5 py-0.5 rounded-full shrink-0 border border-[var(--glass-border-subtle)] bg-[var(--glass-tab-bg)] text-[var(--color-text-secondary)]`}>
              {item.roleLabel}
            </span>
          ) : null}
        </div>
      </button>
    );
  };

  const renderHelpCard = (expanded = false) => (
    <div className={expanded ? "flex h-full min-h-0 flex-col" : promptShellClass}>
      <div className={expanded ? "flex items-start justify-between gap-3" : promptHeaderClass}>
        <div className="min-w-0">
          <div className={`text-sm font-semibold ${promptHeaderTextClass}`}>{t("guidance.helpTitle")}</div>
          <div className={`text-[11px] ${promptHintClass}`}>{t("guidance.helpHint")}</div>
        </div>
        <div className="flex items-center gap-2 shrink-0">
          {!expanded ? (
            <button
              type="button"
              className={secondaryButtonClass("sm")}
              onClick={() => setExpandedKind("help")}
              disabled={busy}
              title={t("guidance.expandTitle")}
            >
              {t("guidance.expand")}
            </button>
          ) : null}
          {renderSourceBadge("help")}
        </div>
      </div>

      <div className={expanded ? "mt-3 min-h-0 flex flex-1 flex-col" : promptBodyClass(expanded)}>
        {help?.path ? (
          <div className={promptPathClass}>
            <span className="truncate">{help.path}</span>
          </div>
        ) : null}

        <div className={`${expanded ? "min-h-0 flex flex-1 flex-col" : ""}`}>
          <div className={`flex items-start justify-between gap-4 ${expanded ? "pb-4" : "mb-4"}`}>
            <div className="min-w-0">
              <div className="max-w-[54ch] text-[11px] leading-5 text-[var(--color-text-tertiary)]">
                {t("guidance.helpEditorHint", "Structured mode edits common, role, and actor notes; raw mode keeps full-file control.")}
              </div>
            </div>
            <div className={`${segmentedControlClass} ${expanded ? "shrink-0" : ""}`}>
              <button
                type="button"
                className={`px-3 py-1.5 text-xs rounded-full transition-colors ${
                  helpViewMode === "structured"
                    ? "bg-[var(--color-accent-primary)] text-[var(--color-text-inverse)]"
                    : "text-[var(--color-text-secondary)] hover:bg-[var(--glass-tab-bg-hover)]"
                }`}
                onClick={() => setHelpViewMode("structured")}
              >
                {t("guidance.structuredView", "Structured")}
              </button>
              <button
                type="button"
                className={`px-3 py-1.5 text-xs rounded-full transition-colors ${
                  helpViewMode === "raw"
                    ? "bg-[var(--color-accent-primary)] text-[var(--color-text-inverse)]"
                    : "text-[var(--color-text-secondary)] hover:bg-[var(--glass-tab-bg-hover)]"
                }`}
                onClick={() => setHelpViewMode("raw")}
              >
                {t("guidance.rawView", "Raw Markdown")}
              </button>
            </div>
          </div>

          {helpViewMode === "structured" ? (
            <div className={`mt-5 grid grid-cols-1 gap-5 ${expanded ? "min-h-0 flex-1 xl:grid-cols-[256px_minmax(0,1fr)]" : "items-start xl:grid-cols-[228px_minmax(0,1fr)]"}`}>
              <div className={`${navigationPanelClass} ${expanded ? "min-h-0 flex flex-col" : "space-y-2.5"}`}>
                <div className={expanded ? `min-h-0 flex-1 space-y-4 ${settingsScrollAreaClass}` : "space-y-4"}>
                  <div className="space-y-2.5">
                    <div className={navSectionTitleClass}>{t("guidance.commonAndRolesTitle", "Shared Scopes")}</div>
                    {renderHelpScopeButton(commonScope)}
                    {renderHelpScopeButton(foremanScope)}
                    {renderHelpScopeButton(peerScope)}
                  </div>

                  <div className={`pt-3 border-t ${isDark ? "border-white/8" : "border-gray-200"}`}>
                    <div className={navSectionTitleClass}>
                      {t("guidance.actorNotesTitle", "Actor Notes")}
                    </div>
                    {actorScopes.length ? (
                      <div className={expanded ? "space-y-2" : `space-y-2 max-h-[360px] ${settingsScrollAreaClass}`}>
                        {actorScopes.map((item) => renderHelpScopeButton(item))}
                      </div>
                    ) : (
                      <div className="rounded-lg border border-dashed border-[var(--glass-border-subtle)] px-3 py-4 text-sm text-[var(--color-text-muted)]">
                        {t("guidance.noActorsForStructuredHelp", "No actors available in this group yet.")}
                      </div>
                    )}
                  </div>

                  {orphanActorScopes.length ? (
                    <div className={`pt-3 border-t ${isDark ? "border-white/8" : "border-gray-200"}`}>
                      <div className={navSectionTitleClass}>
                        {t("guidance.orphanActorNotesTitle", "Other actor notes")}
                      </div>
                      <div className={expanded ? "space-y-2" : `space-y-2 max-h-[220px] ${settingsScrollAreaClass}`}>
                        {orphanActorScopes.map((item) => renderHelpScopeButton(item))}
                      </div>
                    </div>
                  ) : null}
                </div>
              </div>

              <div className={`${workspacePanelClass} ${expanded ? "min-h-0 flex flex-col" : ""}`}>
                <div className="mb-4 flex items-start gap-4">
                  <div className="min-w-0">
                    <div className={`text-[11px] font-medium uppercase tracking-[0.16em] ${isDark ? "text-white/44" : "text-gray-500"}`}>
                      {t("guidance.editKind", { kind: selectedHelpScopeItem.title })}
                    </div>
                    <div className={`mt-1 text-sm font-semibold ${isDark ? "text-white" : "text-gray-900"}`}>
                      {selectedHelpScopeItem.title}
                    </div>
                    <div className={`mt-1 text-[11px] ${isDark ? "text-white/40" : "text-gray-500"}`}>
                      {selectedHelpScopeItem.hint}
                    </div>
                  </div>
                  {selectedHelpScopeItem.roleLabel ? (
                    <div className={`ml-auto shrink-0 rounded-full px-2.5 py-1 text-[10px] ${isDark ? "bg-white/[0.05] text-white/62" : "bg-black/[0.05] text-gray-600"}`}>
                      {selectedHelpScopeItem.roleLabel}
                    </div>
                  ) : null}
                </div>

                <div className={`${editorSurfaceSoftClass} ${expanded ? "min-h-0 flex flex-1 flex-col" : ""}`}>
                  <textarea
                    className={`${editorTextareaClass} font-mono text-[12px] ${expanded ? "min-h-[440px] flex-1" : ""}`}
                    style={expanded ? undefined : { minHeight: 320, maxHeight: "44vh" }}
                    value={selectedHelpScopeItem.value}
                    onChange={(e) => updateSelectedHelpScopeValue(e.target.value)}
                    placeholder={selectedHelpScopeItem.placeholder}
                    spellCheck={false}
                  />
                </div>
              </div>
            </div>
          ) : (
            <div className={`mt-5 ${expanded ? "min-h-0 flex flex-1 flex-col" : ""}`}>
              <div className={`${editorSurfaceSoftClass} ${expanded ? "min-h-0 flex flex-1 flex-col" : ""}`}>
                <div className="mb-4 flex items-center justify-between gap-3">
                  <label className={labelClass(isDark)}>{t("guidance.markdown")}</label>
                  <div className={editorMetaBadgeClass}>Raw</div>
                </div>
                <textarea
                  className={`${editorTextareaClass} font-mono text-[12px] ${expanded ? "min-h-[440px] flex-1" : ""}`}
                  style={expanded ? undefined : { minHeight: 320, maxHeight: "44vh" }}
                  value={help?.content || ""}
                  onChange={(e) => setHelpContentRaw(e.target.value)}
                  spellCheck={false}
                />
              </div>
            </div>
          )}
        </div>
      </div>

      {renderPromptActions("help", expanded)}
    </div>
  );

  return (
    <div className="space-y-3">
      {err ? <div className={`text-sm ${isDark ? "text-rose-300" : "text-red-600"}`}>{err}</div> : null}

      <div className={overridesHintClass}>
        <Trans i18nKey="guidance.overridesHint" ns="settings" components={[<span className="font-mono" />]} />
      </div>

      {renderPreambleCard()}
      {renderHelpCard()}

      {expandedKind
        ? (
          <BodyPortal>
            <div
              key={expandedKind}
              className="fixed inset-0 z-[1000] animate-fade-in"
              role="dialog"
              aria-modal="true"
              onPointerDown={(e) => {
                if (e.target === e.currentTarget) setExpandedKind(null);
              }}
            >
              <div className="absolute inset-0 glass-overlay" />
              <div className={settingsDialogPanelClass("xl")}>
                <div className="flex shrink-0 justify-end border-b border-[var(--glass-border-subtle)] px-3 py-2 sm:px-4 sm:py-3">
                  <button type="button" className={secondaryButtonClass("sm")} onClick={() => setExpandedKind(null)}>
                    {t("common:close")}
                  </button>
                </div>
                <div className={settingsDialogBodyClass}>
                  {expandedKind === "help" ? renderHelpCard(true) : renderPreambleCard(true)}
                </div>
              </div>
            </div>
          </BodyPortal>
          )
        : null}
    </div>
  );
}
