import { classNames } from "../../utils/classNames";
import type { MessageMentionToken } from "../../utils/mentionHighlight";
import { LazyMarkdownRenderer } from "../LazyMarkdownRenderer";
import { MessageMentionText } from "./MessageMentionText";

function PlainMessageText({
  text,
  className,
  mentionTokens,
}: {
  text: string;
  className?: string;
  mentionTokens: readonly MessageMentionToken[];
}) {
  return (
    <div
      className={classNames(
        "break-words whitespace-pre-wrap text-[var(--color-text-primary)] [overflow-wrap:anywhere]",
        className,
      )}
    >
      <MessageMentionText text={text} mentionTokens={mentionTokens} />
    </div>
  );
}

export function MessageContent(props: {
  fallbackText: string;
  shouldRenderMarkdown: boolean;
  isDark: boolean;
  mentionTokens: readonly MessageMentionToken[];
}) {
  const plainText = (
    <PlainMessageText
      text={props.fallbackText}
      className="max-w-full"
      mentionTokens={props.mentionTokens}
    />
  );
  if (!props.shouldRenderMarkdown) return plainText;
  return (
    <LazyMarkdownRenderer
      content={props.fallbackText}
      isDark={props.isDark}
      invertText={false}
      enableMermaid
      className="max-w-full break-words text-[var(--color-text-primary)] [overflow-wrap:anywhere]"
      mentionTokens={props.mentionTokens}
      fallback={plainText}
    />
  );
}
