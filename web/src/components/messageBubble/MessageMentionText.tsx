import { useMemo, type ReactNode } from "react";
import { Highlighter } from "../ui/highlighter";
import {
  findMessageMentionRanges,
  messageMentionColor,
  type MessageMentionRange,
} from "../../utils/mentionHighlight";
import { useSystemReducedMotion, useUiStylePrefs } from "../../utils/uiStyle";

function HighlightedMessageText({
  text,
  ranges,
  animate: allowAnimation = true,
}: {
  text: string;
  ranges: MessageMentionRange[];
  animate?: boolean;
}) {
  const reducedMotion = useSystemReducedMotion();
  const { lcdMotion } = useUiStylePrefs();
  const animate = allowAnimation && !reducedMotion && lcdMotion !== "off";
  const content = useMemo(() => {
    const parts: ReactNode[] = [];
    let cursor = 0;
    for (const range of ranges) {
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
          data-message-mention={range.kind}
        >
          {text.slice(range.start, range.end)}
        </Highlighter>,
      );
      cursor = range.end;
    }
    parts.push(text.slice(cursor));
    return parts;
  }, [animate, ranges, text]);
  return <>{content}</>;
}

export function MessageMentionText({
  text,
  knownTokens,
  animate,
}: {
  text: string;
  knownTokens: string[];
  animate?: boolean;
}) {
  const ranges = useMemo(() => findMessageMentionRanges(text, knownTokens), [knownTokens, text]);
  if (!ranges.length) return <>{text}</>;
  return <HighlightedMessageText text={text} ranges={ranges} animate={animate} />;
}
