import { useMemo, type ReactNode } from "react";
import { Highlighter } from "@/components/ui/highlighter";
import type {
  ComposerAgentMentionToken,
  ComposerGroupMentionToken,
} from "../../hooks/composerGroupMentions";
import { classNames } from "../../utils/classNames";
import { messageMentionColor } from "../../utils/mentionHighlight";
import { useSystemReducedMotion, useUiStylePrefs } from "../../utils/uiStyle";

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
  const reducedMotion = useSystemReducedMotion();
  const { lcdMotion } = useUiStylePrefs();
  const animate = !reducedMotion && lcdMotion !== "off";
  const content = useMemo(() => {
    const ranges = [
      ...groupTokens.map((token) => ({ ...token, kind: "group" as const })),
      ...agentTokens.map((token) => ({ ...token, kind: "agent" as const })),
    ].sort((a, b) => a.start - b.start);
    const parts: ReactNode[] = [];
    let cursor = 0;
    for (const range of ranges) {
      if (range.start < cursor || range.end > text.length) continue;
      parts.push(text.slice(cursor, range.start));
      parts.push(
        <Highlighter
          key={`${range.kind}:${range.start}:${range.end}`}
          action="highlight"
          color={messageMentionColor(range.kind)}
          animationDuration={300}
          iterations={1}
          padding={0}
          animate={animate}
          className="inline"
        >
          {text.slice(range.start, range.end)}
        </Highlighter>,
      );
      cursor = range.end;
    }
    parts.push(text.slice(cursor));
    return ranges.length ? parts : null;
  }, [agentTokens, animate, groupTokens, text]);

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
