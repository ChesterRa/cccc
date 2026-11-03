import { classNames } from "../../utils/classNames";
import { LazyMarkdownRenderer } from "../LazyMarkdownRenderer";
import { MessageMentionText } from "./MessageMentionText";
import { MarkdownMessageContent } from "./MarkdownMessageContent";

function PlainMessageText({
  text,
  className,
  knownTokens,
  animate,
}: {
  text: string;
  className?: string;
  knownTokens: string[];
  animate: boolean;
}) {
  return (
    <div
      className={classNames(
        "break-words whitespace-pre-wrap text-[var(--color-text-primary)] [overflow-wrap:anywhere]",
        className,
      )}
    >
      <MessageMentionText text={text} knownTokens={knownTokens} animate={animate} />
    </div>
  );
}

export function MessageContent(props: {
  fallbackText: string;
  shouldRenderMarkdown: boolean;
  isDark: boolean;
  knownTokens: string[];
  animateMentions: boolean;
}) {
  if (!props.shouldRenderMarkdown) {
    return (
      <PlainMessageText
        text={props.fallbackText}
        className="max-w-full"
        knownTokens={props.knownTokens}
        animate={props.animateMentions}
      />
    );
  }
  if (!/[@#]/u.test(props.fallbackText)) {
    return (
      <LazyMarkdownRenderer
        content={props.fallbackText}
        isDark={props.isDark}
        invertText={false}
        enableMermaid
        className="max-w-full break-words text-[var(--color-text-primary)] [overflow-wrap:anywhere]"
        fallback={
          <PlainMessageText
            text={props.fallbackText}
            className="max-w-full"
            knownTokens={props.knownTokens}
            animate={props.animateMentions}
          />
        }
      />
    );
  }
  return (
    <MarkdownMessageContent
      text={props.fallbackText}
      isDark={props.isDark}
      knownTokens={props.knownTokens}
      animate={props.animateMentions}
      enableMermaid
      fallback={
        <PlainMessageText
          text={props.fallbackText}
          className="max-w-full"
          knownTokens={props.knownTokens}
          animate={props.animateMentions}
        />
      }
    />
  );
}
