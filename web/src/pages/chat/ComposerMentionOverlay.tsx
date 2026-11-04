import { useMemo, type ReactNode } from "react";
import type {
  ComposerAgentMentionToken,
  ComposerGroupMentionToken,
} from "../../hooks/composerGroupMentions";
import { getActorAccentIndex } from "../../types";
import { classNames } from "../../utils/classNames";

/** Marks the selected references behind the composer text, which stays in the textarea above. */
export function ComposerMentionOverlay({
  text,
  groupTokens,
  agentTokens,
  scrollTop,
  compact,
  minHeight,
  maxHeight,
  fontSize,
  lineHeight,
}: {
  text: string;
  groupTokens: ComposerGroupMentionToken[];
  agentTokens: ComposerAgentMentionToken[];
  scrollTop: number;
  compact: boolean;
  minHeight: string;
  maxHeight: string;
  fontSize: number;
  lineHeight: number;
}) {
  const content = useMemo(() => {
    const ranges = [
      ...groupTokens.map((token) => ({ ...token, kind: "group" as const, accent: undefined })),
      // Selectors such as @all have no name colour; an agent's tint matches its name.
      ...agentTokens.map((token) => ({
        ...token,
        kind: "agent" as const,
        accent: token.actorId.startsWith("@") ? undefined : getActorAccentIndex(token.actorId),
      })),
    ].sort((a, b) => a.start - b.start);
    const parts: ReactNode[] = [];
    let cursor = 0;
    for (const range of ranges) {
      if (range.start < cursor || range.end > text.length) continue;
      parts.push(text.slice(cursor, range.start));
      parts.push(
        <span
          key={range.start}
          data-composer-mention={range.kind}
          data-mention-accent={range.accent}
        >
          {text.slice(range.start, range.end)}
        </span>,
      );
      cursor = range.end;
    }
    parts.push(text.slice(cursor));
    return cursor ? parts : null;
  }, [agentTokens, groupTokens, text]);

  if (!content) return null;
  return (
    <div
      className={classNames(
        "pointer-events-none absolute inset-0 overflow-hidden whitespace-pre-wrap break-words border-none px-4 text-transparent",
        compact ? "py-2" : "py-3",
      )}
      style={{ minHeight, maxHeight, fontSize: `${fontSize}px`, lineHeight: `${lineHeight}px` }}
      aria-hidden="true"
      data-composer-mention-overlay
    >
      <div style={{ transform: `translateY(-${scrollTop}px)` }}>{content}</div>
    </div>
  );
}
