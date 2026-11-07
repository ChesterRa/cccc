import { useMemo, type ReactNode } from "react";
import {
  findMessageLinkRanges,
  findMessageMentionRanges,
  type MessageMentionToken,
} from "../../utils/mentionHighlight";
import { findMessagePathRanges } from "../../utils/messagePaths";

export function MessageMentionText({
  text,
  mentionTokens,
  pathRoot,
}: {
  text: string;
  mentionTokens: readonly MessageMentionToken[];
  pathRoot?: string;
}) {
  const content = useMemo(() => {
    const mentions = findMessageMentionRanges(text, mentionTokens).map((range) => ({
      ...range,
      type: "mention" as const,
    }));
    const links = findMessageLinkRanges(text).map((range) => ({ ...range, type: "link" as const }));
    const paths = findMessagePathRanges(text, pathRoot || "").map((range) => ({
      ...range,
      type: "path" as const,
    }));
    // A mention inside a path belongs to the path, not to a separate span.
    const visibleMentions = mentions.filter(
      (mention) => !paths.some((path) => mention.start < path.end && path.start < mention.end),
    );
    const ranges = [...visibleMentions, ...links, ...paths].sort((a, b) => a.start - b.start);
    if (!ranges.length) return text;
    const parts: ReactNode[] = [];
    let cursor = 0;
    for (const range of ranges) {
      parts.push(text.slice(cursor, range.start));
      parts.push(
        range.type === "link" ? (
          <a
            key={range.start}
            href={range.href}
            target="_blank"
            rel="noopener noreferrer"
            className="break-all underline underline-offset-2 hover:opacity-80"
          >
            {text.slice(range.start, range.end)}
          </a>
        ) : range.type === "path" ? (
          <span
            key={range.start}
            role="button"
            tabIndex={0}
            data-message-path={range.path}
            className="cursor-pointer break-all underline underline-offset-2 hover:opacity-80"
          >
            {text.slice(range.start, range.end)}
          </span>
        ) : (
          <span
            key={range.start}
            data-message-mention={range.kind}
            data-mention-accent={range.accent}
          >
            {text.slice(range.start, range.end)}
          </span>
        ),
      );
      cursor = range.end;
    }
    parts.push(text.slice(cursor));
    return parts;
  }, [mentionTokens, pathRoot, text]);
  return <>{content}</>;
}
