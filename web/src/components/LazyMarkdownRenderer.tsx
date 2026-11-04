import { lazy, Suspense, type ReactNode } from "react";
import type { MessageMentionToken } from "../utils/mentionHighlight";

const MarkdownRenderer = lazy(() =>
  import("./MarkdownRenderer").then((module) => ({ default: module.MarkdownRenderer })),
);

type LazyMarkdownRendererProps = {
  content: string;
  isDark?: boolean;
  className?: string;
  invertText?: boolean;
  enableMermaid?: boolean;
  fallback?: ReactNode;
  resolveUrl?: (url: string, kind: "image" | "link") => string;
  onRendered?: () => void;
  mentionTokens?: readonly MessageMentionToken[];
};

export function LazyMarkdownRenderer({
  content,
  isDark,
  className,
  invertText,
  enableMermaid = false,
  fallback = null,
  resolveUrl,
  onRendered,
  mentionTokens,
}: LazyMarkdownRendererProps) {
  return (
    <Suspense fallback={fallback}>
      <MarkdownRenderer
        content={content}
        isDark={isDark}
        className={className}
        invertText={invertText}
        enableMermaid={enableMermaid}
        resolveUrl={resolveUrl}
        onRendered={onRendered}
        mentionTokens={mentionTokens}
      />
    </Suspense>
  );
}
