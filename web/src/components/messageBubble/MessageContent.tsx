import { classNames } from "../../utils/classNames";
import { LazyMarkdownRenderer } from "../LazyMarkdownRenderer";

function PlainMessageText({ text, className }: { text: string; className?: string }) {
  return (
    <div
      className={classNames(
        "break-words whitespace-pre-wrap text-[var(--color-text-primary)] [overflow-wrap:anywhere]",
        className,
      )}
    >
      {text}
    </div>
  );
}

export function MessageContent(props: {
  fallbackText: string;
  shouldRenderMarkdown: boolean;
  isDark: boolean;
}) {
  if (!props.shouldRenderMarkdown) {
    return <PlainMessageText text={props.fallbackText} className="max-w-full" />;
  }
  return (
    <LazyMarkdownRenderer
      content={props.fallbackText}
      isDark={props.isDark}
      invertText={false}
      enableMermaid
      className="max-w-full break-words text-[var(--color-text-primary)] [overflow-wrap:anywhere]"
      fallback={<PlainMessageText text={props.fallbackText} className="max-w-full" />}
    />
  );
}
