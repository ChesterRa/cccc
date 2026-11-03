import { useLayoutEffect, useRef, type Dispatch, type RefObject, type SetStateAction } from "react";
import {
  createComposerAgentMentionToken,
  createComposerGroupMentionToken,
  pruneComposerAgentMentionTokens,
  pruneComposerGroupMentionTokens,
  resolveControlledComposerMentionContext,
  type ComposerAgentMentionToken,
  type ComposerGroupMentionToken,
} from "../../hooks/composerGroupMentions";
import { useComposerStore } from "../../stores/useComposerStore";
import {
  getComposerGroupMentionInsertToken,
  type ComposerMentionKind,
  type ComposerMentionSuggestion,
} from "./chatMentionSuggestions";
import {
  findComposerMentionAtCaret,
  getComposerTextEdit,
  rebaseComposerMentionTokens,
  retainComposerMentionSuffix,
  replaceComposerMention,
  type ComposerInputSelection,
} from "./composerMentionInput";

type Options = {
  text: string;
  composerRef: RefObject<HTMLTextAreaElement | null>;
  setText: Dispatch<SetStateAction<string>>;
  setGroupTokens: Dispatch<SetStateAction<ComposerGroupMentionToken[]>>;
  setAgentTokens: Dispatch<SetStateAction<ComposerAgentMentionToken[]>>;
  setShowMenu: Dispatch<SetStateAction<boolean>>;
  setSelectedIndex: Dispatch<SetStateAction<number>>;
  setFilter: Dispatch<SetStateAction<string>>;
  setKind: Dispatch<SetStateAction<ComposerMentionKind>>;
  setActorScope: Dispatch<SetStateAction<"selected" | "destination">>;
  setTargetGroupId: Dispatch<SetStateAction<string>>;
  setTriggerIndex?: Dispatch<SetStateAction<number | null>>;
  updateMenuPosition: (textToTrigger: string) => void;
};

export function useComposerMentionInput({
  text,
  composerRef,
  setText,
  setGroupTokens,
  setAgentTokens,
  setShowMenu,
  setSelectedIndex,
  setFilter,
  setKind,
  setActorScope,
  setTargetGroupId,
  setTriggerIndex,
  updateMenuPosition,
}: Options) {
  const selectionRef = useRef<ComposerInputSelection | null>(null);
  const pendingCaretRef = useRef<{ text: string; caret: number } | null>(null);
  const activeMentionRef = useRef<ReturnType<typeof findComposerMentionAtCaret>>(null);

  useLayoutEffect(() => {
    const pending = pendingCaretRef.current;
    const input = composerRef.current;
    if (!pending || !input) return;
    if (pending.text !== text) {
      pendingCaretRef.current = null;
      return;
    }
    input.focus();
    input.setSelectionRange(pending.caret, pending.caret);
    selectionRef.current = { text: pending.text, start: pending.caret, end: pending.caret };
    pendingCaretRef.current = null;
  }, [composerRef, text]);

  const rememberSelection = (input: HTMLTextAreaElement) => {
    selectionRef.current = {
      text: input.value,
      start: input.selectionStart,
      end: input.selectionEnd,
    };
  };

  const clearActiveMention = () => {
    activeMentionRef.current = null;
    setTriggerIndex?.(null);
  };

  const updateMenu = (value: string, caret: number, selectionEnd = caret) => {
    const trigger = findComposerMentionAtCaret(value, caret, selectionEnd);
    const previous = activeMentionRef.current;
    activeMentionRef.current = trigger;
    setTriggerIndex?.(trigger?.start ?? null);
    if (!trigger) {
      setShowMenu(false);
      setActorScope("selected");
      setTargetGroupId("");
      setFilter("");
      return;
    }

    const context =
      trigger.kind === "agent"
        ? resolveControlledComposerMentionContext({
            text: value,
            atIndex: trigger.start,
            tokens: useComposerStore.getState().composerGroupMentionTokens,
          })
        : { scope: "selected" as const, mentionTargetGroupId: "" };
    setKind(trigger.kind);
    setActorScope(context.scope);
    setTargetGroupId(context.mentionTargetGroupId);
    setFilter(trigger.filter);
    setShowMenu(true);
    if (
      !previous ||
      previous.kind !== trigger.kind ||
      previous.start !== trigger.start ||
      previous.end !== trigger.end ||
      previous.filter !== trigger.filter
    )
      setSelectedIndex(0);
    requestAnimationFrame(() => updateMenuPosition(value.slice(0, trigger.start + 1)));
  };

  const updateText = (value: string, selectionStart: number, selectionEnd: number) => {
    const current = useComposerStore.getState();
    const caret = Math.min(selectionStart, value.length);
    const edit = getComposerTextEdit(current.composerText, value, selectionRef.current, caret);
    const trigger = findComposerMentionAtCaret(value, caret, Math.min(selectionEnd, value.length));
    const rebasedGroups = rebaseComposerMentionTokens(current.composerGroupMentionTokens, edit);
    const rebasedAgents = rebaseComposerMentionTokens(current.composerAgentMentionTokens, edit);
    const liveGroups = pruneComposerGroupMentionTokens({ text: value, tokens: rebasedGroups });
    const liveAgents = pruneComposerAgentMentionTokens({ text: value, tokens: rebasedAgents });
    setText(value);
    setGroupTokens(retainComposerMentionSuffix(value, rebasedGroups, liveGroups, trigger?.end));
    setAgentTokens(retainComposerMentionSuffix(value, rebasedAgents, liveAgents, trigger?.end));
    selectionRef.current = { text: value, start: caret, end: caret };
    return caret;
  };

  const selectMention = (selected: ComposerMentionSuggestion | undefined) => {
    if (!selected) return;
    const current = useComposerStore.getState();
    const input = composerRef.current;
    const caret = input?.selectionStart ?? current.composerText.length;
    const trigger = findComposerMentionAtCaret(
      current.composerText,
      caret,
      input?.selectionEnd ?? caret,
    );
    if (!trigger || trigger.kind !== selected.kind) {
      setShowMenu(false);
      clearActiveMention();
      return;
    }
    const label = String(selected.label || selected.value || "").trim();
    const tokenText =
      selected.kind === "agent"
        ? label.startsWith("@")
          ? label
          : `@${label}`
        : getComposerGroupMentionInsertToken(selected);
    const next = replaceComposerMention(current.composerText, trigger, tokenText);
    let groups = pruneComposerGroupMentionTokens({
      text: next.text,
      tokens: rebaseComposerMentionTokens(current.composerGroupMentionTokens, next.edit),
    });
    let agents = pruneComposerAgentMentionTokens({
      text: next.text,
      tokens: rebaseComposerMentionTokens(current.composerAgentMentionTokens, next.edit),
    });
    if (selected.kind === "agent") {
      const context = resolveControlledComposerMentionContext({
        text: current.composerText,
        atIndex: trigger.start,
        tokens: current.composerGroupMentionTokens,
      });
      const token = createComposerAgentMentionToken({
        actorId: selected.value,
        token: tokenText,
        start: next.tokenStart,
        scope: context.scope,
      });
      if (token) agents = [...agents, token];
    } else {
      const token = createComposerGroupMentionToken({
        groupId: selected.value,
        remote: selected.remote,
        token: tokenText,
        start: next.tokenStart,
      });
      if (token) groups = [...groups, token];
    }
    pendingCaretRef.current = { text: next.text, caret: next.caret };
    setText(next.text);
    setGroupTokens(groups);
    setAgentTokens(agents);
    setShowMenu(false);
    setSelectedIndex(0);
    clearActiveMention();
    requestAnimationFrame(() => {
      const textarea = composerRef.current;
      if (!textarea || textarea.value !== next.text || pendingCaretRef.current?.text !== next.text)
        return;
      textarea.focus();
      textarea.setSelectionRange(next.caret, next.caret);
      rememberSelection(textarea);
      pendingCaretRef.current = null;
    });
  };

  return { clearActiveMention, rememberSelection, selectMention, updateMenu, updateText };
}
