import { useMemo, type ReactNode } from "react";
import { findMessageMentionRanges, type MessageMentionToken } from "../../utils/mentionHighlight";

export function MessageMentionText({
  text,
  mentionTokens,
}: {
  text: string;
  mentionTokens: readonly MessageMentionToken[];
}) {
  const content = useMemo(() => {
    const ranges = findMessageMentionRanges(text, mentionTokens);
    if (!ranges.length) return text;
    const parts: ReactNode[] = [];
    let cursor = 0;
    for (const range of ranges) {
      parts.push(text.slice(cursor, range.start));
      parts.push(
        <span
          key={range.start}
          data-message-mention={range.kind}
          data-mention-accent={range.accent}
        >
          {text.slice(range.start, range.end)}
        </span>,
      );
      cursor = range.end;
    }
    parts.push(text.slice(cursor));
    return parts;
  }, [mentionTokens, text]);
  return <>{content}</>;
}
