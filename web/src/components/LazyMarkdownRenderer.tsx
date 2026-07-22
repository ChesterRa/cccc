import { lazy, Suspense, type ReactNode } from "react";

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
};

export function LazyMarkdownRenderer({
  content,
  isDark,
  className,
  invertText,
  enableMermaid = false,
  fallback = null,
}: LazyMarkdownRendererProps) {
  return (
    <Suspense fallback={fallback}>
      <MarkdownRenderer
        content={content}
        isDark={isDark}
        className={className}
        invertText={invertText}
        enableMermaid={enableMermaid}
      />
    </Suspense>
  );
}
