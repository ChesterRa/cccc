import { useEffect, useId, useMemo, useRef, useState } from "react";
import { Trans, useTranslation } from "react-i18next";
import * as api from "../../../services/api";
import { useModalA11y } from "../../../hooks/useModalA11y";
import type { Actor } from "../../../types";
import {
  buildHelpMarkdown,
  parseHelpMarkdown,
  type HelpChangedBlock,
  type ParsedHelpMarkdown,
} from "../../../utils/helpMarkdown";
import { BodyPortal } from "../../ui/BodyPortal";
import { SegmentedChoice } from "../../ui/segmented-choice";
import {
  cardClass,
  inputClass,
  primaryButtonClass,
  secondaryButtonClass,
  settingsDialogPanelClass,
} from "./types";
import { SettingsCard, SettingsPage, SettingsRow } from "./SettingsKit";

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

export function GuidanceTab({ isDark, groupId }: { isDark: boolean; groupId?: string }) {
  const { t } = useTranslation("settings");
  const [busy, setBusy] = useState(false);
  const [err, setErr] = useState("");
  const [hint, setHint] = useState("");
  const [prompts, setPrompts] = useState<Record<PromptKind, PromptInfo> | null>(null);
  const [actors, setActors] = useState<Actor[]>([]);
  const [expandedKind, setExpandedKind] = useState<PromptKind | null>(null);
  const [helpViewMode, setHelpViewMode] = useState<HelpViewMode>("structured");
  const [helpStructured, setHelpStructured] = useState<ParsedHelpMarkdown>(EMPTY_HELP);
  const [helpTouchedRaw, setHelpTouchedRaw] = useState(false);
  const [helpChangedBlocks, setHelpChangedBlocks] = useState<HelpChangedBlock[]>([]);
  const [selectedHelpScope, setSelectedHelpScope] = useState<HelpScopeId>("common");
  const { modalRef: expandedModalRef } = useModalA11y(expandedKind !== null, () =>
    setExpandedKind(null),
  );
  const groupVisit = useRef({ groupId, request: 0 });
  if (groupVisit.current.groupId !== groupId) groupVisit.current = { groupId, request: 0 };

  const ids = { preamble: useId(), helpMode: useId(), helpEditor: useId() };
  // The expanded dialog renders a second copy of a card, so each copy needs its own ids
  // for labels to reach their own control.
  const idFor = (key: keyof typeof ids, expanded: boolean) =>
    expanded ? `${ids[key]}expanded` : ids[key];

  const liveDraft = useRef({ groupId, prompts });
  liveDraft.current = { groupId, prompts };

  const actorIds = useMemo(
    () => actors.map((actor) => String(actor.id || "").trim()).filter(Boolean),
    [actors],
  );
  const actorIdSet = useMemo(() => new Set(actorIds), [actorIds]);
  const orphanActorIds = useMemo(
    () =>
      Object.keys(helpStructured.actorNotes)
        .filter((actorId) => !actorIdSet.has(actorId))
        .sort(),
    [helpStructured.actorNotes, actorIdSet],
  );

  const syncHelpState = (content: string) => {
    setHelpStructured(parseHelpMarkdown(content));
  };

  const beginOperation = () => {
    const visit = groupVisit.current;
    const request = ++visit.request;
    setBusy(true);
    setErr("");
    setHint("");
    return () => groupVisit.current === visit && visit.request === request;
  };

  const load = async () => {
    if (!groupId) return;
    const isCurrent = beginOperation();
    try {
      const [promptsResp, actorsResp] = await Promise.all([
        api.fetchGroupPrompts(groupId),
        api.fetchActors(groupId, false),
      ]);
      if (!isCurrent()) return;
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
      const nextActors = actorsResp.ok ? actorsResp.result?.actors || [] : [];
      setPrompts({ preamble: p, help: h });
      setActors(nextActors);
      syncHelpState(String(h.content || ""));
      setHelpTouchedRaw(false);
      setHelpChangedBlocks([]);
      setHelpViewMode("structured");
      setSelectedHelpScope("common");
    } catch {
      if (!isCurrent()) return;
      setErr(t("guidance.failedToLoad"));
      setPrompts(null);
      setActors([]);
    } finally {
      if (isCurrent()) setBusy(false);
    }
  };

  useEffect(() => {
    const visit = groupVisit.current;
    setPrompts(null);
    setActors([]);
    setHelpStructured(EMPTY_HELP);
    setHelpTouchedRaw(false);
    setHelpChangedBlocks([]);
    setSelectedHelpScope("common");
    setExpandedKind(null);
    setErr("");
    setHint("");
    setBusy(false);
    if (groupId) void load();
    return () => {
      visit.request += 1;
    };
    // eslint-disable-next-line react-hooks/exhaustive-deps -- reload when group changes.
  }, [groupId]);

  const setPromptContent = (kind: PromptKind, content: string) => {
    setHint("");
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

  const acceptSavedPrompt = (kind: PromptKind, saved: PromptInfo, submitted: string) => {
    if (liveDraft.current.groupId !== groupId) return;
    const editedWhileSaving = liveDraft.current.prompts?.[kind].content !== submitted;
    setPrompts((current) =>
      current
        ? {
            ...current,
            [kind]: {
              ...saved,
              content: editedWhileSaving ? current[kind].content : saved.content,
            },
          }
        : current,
    );
    if (kind === "help" && !editedWhileSaving) {
      syncHelpState(String(saved.content || ""));
      setHelpTouchedRaw(false);
      setHelpChangedBlocks([]);
    }
    if (!editedWhileSaving) setHint(t("saveFeedback.saved"));
  };

  const savePrompt = async (kind: PromptKind) => {
    if (!groupId || !prompts) return;
    const isCurrent = beginOperation();
    try {
      const resp = await api.updateGroupPrompt(groupId, kind, prompts[kind].content || "");
      if (!isCurrent()) return;
      if (!resp.ok) {
        setErr(resp.error?.message || t("guidance.failedToSave", { kind }));
        return;
      }
      acceptSavedPrompt(kind, resp.result, prompts[kind].content || "");
    } catch {
      if (isCurrent()) setErr(t("guidance.failedToSave", { kind }));
    } finally {
      if (isCurrent()) setBusy(false);
    }
  };

  const saveHelp = async () => {
    if (!groupId || !prompts) return;
    const isCurrent = beginOperation();
    try {
      const resp = await api.updateGroupPrompt(
        groupId,
        "help",
        prompts.help.content || "",
        helpTouchedRaw
          ? { editorMode: "raw" }
          : { editorMode: "structured", changedBlocks: helpChangedBlocks },
      );
      if (!isCurrent()) return;
      if (!resp.ok) {
        setErr(resp.error?.message || t("guidance.failedToSave", { kind: "help" }));
        return;
      }
      acceptSavedPrompt("help", resp.result, prompts.help.content || "");
    } catch {
      if (isCurrent()) setErr(t("guidance.failedToSave", { kind: "help" }));
    } finally {
      if (isCurrent()) setBusy(false);
    }
  };

  const resetPrompt = async (kind: PromptKind) => {
    if (!groupId || !prompts) return;
    const filename = prompts[kind]?.filename || kind;
    const ok = window.confirm(t("automation.resetGuidanceConfirm", { kind, filename }));
    if (!ok) return;
    const isCurrent = beginOperation();
    try {
      const resp = await api.resetGroupPrompt(groupId, kind);
      if (!isCurrent()) return;
      if (!resp.ok) {
        setErr(resp.error?.message || t("guidance.failedToReset", { kind }));
        return;
      }
      acceptSavedPrompt(kind, resp.result, prompts[kind].content || "");
    } catch {
      if (isCurrent()) setErr(t("guidance.failedToReset", { kind }));
    } finally {
      if (isCurrent()) setBusy(false);
    }
  };

  if (!groupId) {
    return (
      <div className={cardClass(isDark)}>
        <div className="text-sm text-[var(--color-text-secondary)]">
          {t("guidance.openFromGroup")}
        </div>
      </div>
    );
  }

  const preamble = prompts?.preamble;
  const help = prompts?.help;
  const helpSource = help?.source || "builtin";
  const preambleSource = preamble?.source || "builtin";
  const editorClass = (expanded: boolean, minHeight: string) =>
    `${inputClass(isDark)} w-full font-mono text-xs leading-6 ${expanded ? "min-h-[60vh] resize-none" : `${minHeight} resize-y`}`;
  const captionClass = "text-[11px] font-medium text-[var(--color-text-muted)]";

  const renderSourceRow = (kind: PromptKind) => {
    const source = kind === "help" ? helpSource : preambleSource;
    const path = kind === "help" ? help?.path : preamble?.path;
    return (
      <SettingsRow
        label={t("common:status")}
        help={path ? <span className="font-mono break-all">{path}</span> : undefined}
      >
        <span
          className={`inline-flex items-center rounded-full border px-2.5 py-1 text-xs font-medium ${
            source === "home"
              ? "border-[var(--color-text-primary)]/20 text-[var(--color-text-primary)]"
              : "border-[var(--glass-border-subtle)] bg-[var(--glass-tab-bg)] text-[var(--color-text-secondary)]"
          }`}
          data-guidance-source={source}
        >
          {source === "home" ? t("guidance.overrideBadge") : t("guidance.builtinBadge")}
        </span>
      </SettingsRow>
    );
  };

  const renderPromptActions = (kind: PromptKind, expanded: boolean) => {
    const source = kind === "help" ? helpSource : preambleSource;
    const handleSave = kind === "help" ? () => void saveHelp() : () => void savePrompt("preamble");
    return (
      <>
        {!expanded ? (
          <button
            type="button"
            className={secondaryButtonClass("sm")}
            onClick={() => setExpandedKind(kind)}
            disabled={busy}
            title={t("guidance.expandTitle")}
          >
            {t("guidance.expand")}
          </button>
        ) : null}
        <button
          type="button"
          className={secondaryButtonClass("sm")}
          onClick={() => void resetPrompt(kind)}
          disabled={busy || source !== "home"}
          title={source === "home" ? t("guidance.resetHint") : t("guidance.noOverride")}
        >
          {t("common:reset")}
        </button>
        <button
          type="button"
          className={secondaryButtonClass("sm")}
          onClick={() => void load()}
          disabled={busy}
          title={t("guidance.discardChanges")}
        >
          {t("guidance.discardChanges")}
        </button>
        <button
          type="button"
          className={primaryButtonClass(busy)}
          onClick={handleSave}
          disabled={busy}
        >
          {t("common:save")}
        </button>
      </>
    );
  };

  const renderPreambleCard = (expanded = false) => (
    <SettingsCard
      title={t("guidance.preambleTitle")}
      description={t("guidance.preambleHint")}
      actions={renderPromptActions("preamble", expanded)}
    >
      {renderSourceRow("preamble")}
      <SettingsRow label={t("guidance.markdown")} htmlFor={idFor("preamble", expanded)} stacked>
        <textarea
          id={idFor("preamble", expanded)}
          className={editorClass(expanded, "min-h-[220px]")}
          value={preamble?.content || ""}
          onChange={(e) => setPromptContent("preamble", e.target.value)}
          spellCheck={false}
        />
      </SettingsRow>
    </SettingsCard>
  );

  const commonScope = {
    id: "common" as HelpScopeId,
    title: t("guidance.commonNotesTitle", "Common Notes"),
    hint: t("guidance.commonNotesHint", "Untagged help content shared by all actors."),
    placeholder: t(
      "guidance.commonNotesPlaceholder",
      "Keep shared guidance, workflow details, and appendices here...",
    ),
    value: helpStructured.common,
    roleLabel: undefined as string | undefined,
    isOrphan: false,
  };

  const foremanScope = {
    id: "role:foreman" as HelpScopeId,
    title: t("guidance.foremanNotesTitle", "Foreman Notes"),
    hint: t("guidance.foremanNotesHint", "Only foreman actors receive this scoped block."),
    placeholder: t(
      "guidance.foremanNotesPlaceholder",
      "Own outcome quality, review peer outputs, and keep shared direction coherent...",
    ),
    value: helpStructured.foreman,
    roleLabel: undefined as string | undefined,
    isOrphan: false,
  };

  const peerScope = {
    id: "role:peer" as HelpScopeId,
    title: t("guidance.peerNotesTitle", "Peer Notes"),
    hint: t("guidance.peerNotesHint", "Only peer actors receive this scoped block."),
    placeholder: t(
      "guidance.peerNotesPlaceholder",
      "Report risks early, deliver verifiable outputs, and say when the direction is wrong...",
    ),
    value: helpStructured.peer,
    roleLabel: undefined as string | undefined,
    isOrphan: false,
  };

  const actorScopes = actors.map((actor) => {
    const actorId = String(actor.id || "").trim();
    const note = String(helpStructured.actorNotes[actorId] || "");
    const roleLabel =
      String(actor.role || t("guidance.unknownRole", "Unknown")).trim() ||
      t("guidance.unknownRole", "Unknown");
    return {
      id: `actor:${actorId}` as HelpScopeId,
      title: displayActorName(actor),
      hint: t(
        "guidance.actorNotesHint",
        "Local notes for specific actors. This is the same source edited from the actor modal shortcut.",
      ),
      placeholder: t(
        "guidance.actorNotePlaceholder",
        "Describe only this actor's local responsibilities, boundaries, and preferred behavior...",
      ),
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
      hint: t(
        "guidance.actorNotesHint",
        "Local notes for specific actors. This is the same source edited from the actor modal shortcut.",
      ),
      placeholder: t(
        "guidance.orphanActorNotePlaceholder",
        "Keep or clean this leftover note for an actor that no longer exists...",
      ),
      value: note,
      roleLabel: t("guidance.orphanActorRole", "No longer in group"),
      isOrphan: true,
    };
  });

  const selectedHelpScopeItem =
    [commonScope, foremanScope, peerScope, ...actorScopes, ...orphanActorScopes].find(
      (item) => item.id === selectedHelpScope,
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

  const scopeChoice = (caption: string, items: Array<{ id: HelpScopeId; title: string }>) => (
    <div className="flex min-w-0 flex-col items-start gap-1.5">
      <span className={captionClass}>{caption}</span>
      <SegmentedChoice
        fill={false}
        ariaLabel={caption}
        value={selectedHelpScope}
        options={items.map((item) => ({ value: item.id, label: item.title }))}
        onChange={setSelectedHelpScope}
      />
    </div>
  );

  const renderHelpCard = (expanded = false) => (
    <SettingsCard
      title={t("guidance.helpTitle")}
      description={t("guidance.helpHint")}
      actions={renderPromptActions("help", expanded)}
    >
      {renderSourceRow("help")}
      <SettingsRow
        label={t("guidance.helpEditorTitle")}
        help={t(
          "guidance.helpEditorHint",
          "Structured mode edits common, role, and actor notes; raw mode keeps full-file control.",
        )}
        labelId={idFor("helpMode", expanded)}
      >
        <SegmentedChoice
          fill={false}
          labelledBy={idFor("helpMode", expanded)}
          value={helpViewMode}
          options={[
            { value: "structured", label: t("guidance.structuredView", "Structured") },
            { value: "raw", label: t("guidance.rawView", "Raw Markdown") },
          ]}
          onChange={setHelpViewMode}
        />
      </SettingsRow>
      {helpViewMode === "structured" ? (
        <SettingsRow
          label={t("guidance.editKind", { kind: selectedHelpScopeItem.title })}
          help={
            <>
              {selectedHelpScopeItem.hint}
              {selectedHelpScopeItem.roleLabel ? (
                <span className="ml-2 inline-flex rounded-full border border-[var(--glass-border-subtle)] bg-[var(--glass-tab-bg)] px-1.5 text-[10px] text-[var(--color-text-secondary)]">
                  {selectedHelpScopeItem.roleLabel}
                </span>
              ) : null}
            </>
          }
          htmlFor={idFor("helpEditor", expanded)}
          stacked
        >
          <div className="flex w-full min-w-0 flex-col gap-3">
            {scopeChoice(t("guidance.commonAndRolesTitle", "Shared Scopes"), [
              commonScope,
              foremanScope,
              peerScope,
            ])}
            {actorScopes.length ? (
              scopeChoice(t("guidance.actorNotesTitle", "Actor Notes"), actorScopes)
            ) : (
              <div className="flex flex-col gap-1.5">
                <span className={captionClass}>{t("guidance.actorNotesTitle", "Actor Notes")}</span>
                <span className="text-xs text-[var(--color-text-muted)]">
                  {t(
                    "guidance.noActorsForStructuredHelp",
                    "No actors available in this group yet.",
                  )}
                </span>
              </div>
            )}
            {orphanActorScopes.length
              ? scopeChoice(
                  t("guidance.orphanActorNotesTitle", "Other actor notes"),
                  orphanActorScopes,
                )
              : null}
            <textarea
              id={idFor("helpEditor", expanded)}
              className={editorClass(expanded, "min-h-[320px]")}
              style={expanded ? undefined : { maxHeight: "44vh" }}
              value={selectedHelpScopeItem.value}
              onChange={(e) => updateSelectedHelpScopeValue(e.target.value)}
              placeholder={selectedHelpScopeItem.placeholder}
              spellCheck={false}
            />
          </div>
        </SettingsRow>
      ) : (
        <SettingsRow label={t("guidance.markdown")} htmlFor={idFor("helpEditor", expanded)} stacked>
          <textarea
            id={idFor("helpEditor", expanded)}
            className={editorClass(expanded, "min-h-[320px]")}
            style={expanded ? undefined : { maxHeight: "44vh" }}
            value={help?.content || ""}
            onChange={(e) => setHelpContentRaw(e.target.value)}
            spellCheck={false}
          />
        </SettingsRow>
      )}
    </SettingsCard>
  );

  return (
    <SettingsPage
      title={t("tabs.guidance")}
      description={
        <Trans
          i18nKey="guidance.overridesHint"
          ns="settings"
          components={[<span key="override" className="font-mono" />]}
        />
      }
    >
      {hint ? (
        <p role="status" className="text-sm text-[var(--color-accent-success)]">
          {hint}
        </p>
      ) : null}
      {err ? (
        <div role="alert" className="text-sm text-rose-600 dark:text-rose-400">
          {err}
        </div>
      ) : null}

      {renderPreambleCard()}
      {renderHelpCard()}

      {expandedKind ? (
        <BodyPortal>
          <div
            ref={expandedModalRef}
            key={expandedKind}
            className="fixed inset-0 z-[1000] animate-fade-in"
            role="dialog"
            aria-modal="true"
            aria-label={t(
              expandedKind === "help" ? "guidance.helpTitle" : "guidance.preambleTitle",
            )}
            onPointerDown={(e) => {
              if (e.target === e.currentTarget) setExpandedKind(null);
            }}
          >
            <div className="absolute inset-0 glass-overlay" />
            <div className={settingsDialogPanelClass("xl")}>
              <div className="flex shrink-0 justify-end border-b border-[var(--glass-border-subtle)] px-3 py-2 sm:px-4 sm:py-3">
                <button
                  type="button"
                  className={secondaryButtonClass("sm")}
                  onClick={() => setExpandedKind(null)}
                >
                  {t("common:close")}
                </button>
              </div>
              <div className="@container/settings min-h-0 flex-1 overflow-y-auto p-4 sm:p-6 lg:p-7">
                {expandedKind === "help" ? renderHelpCard(true) : renderPreambleCard(true)}
              </div>
            </div>
          </div>
        </BodyPortal>
      ) : null}
    </SettingsPage>
  );
}
