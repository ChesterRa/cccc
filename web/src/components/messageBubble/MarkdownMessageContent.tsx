import { useCallback, useLayoutEffect, useRef, useState, type ReactNode } from "react";
import { LazyMarkdownRenderer } from "../LazyMarkdownRenderer";
import { useSystemReducedMotion, useUiStylePrefs } from "../../utils/uiStyle";
import { highlightMarkdownMentions } from "./markdownMentionHighlights";

export function MarkdownMessageContent({
  text,
  isDark,
  knownTokens,
  fallback,
  enableMermaid,
  animate,
}: {
  text: string;
  isDark: boolean;
  knownTokens: string[];
  fallback: ReactNode;
  enableMermaid: boolean;
  animate: boolean;
}) {
  const rootRef = useRef<HTMLDivElement>(null);
  const [ready, setReady] = useState(false);
  const reducedMotion = useSystemReducedMotion();
  const { lcdMotion } = useUiStylePrefs();
  const onRendered = useCallback(() => setReady(true), []);

  useLayoutEffect(() => {
    const root = rootRef.current?.querySelector<HTMLElement>(".markdown-body");
    if (!ready || !root) return;
    return highlightMarkdownMentions(
      root,
      knownTokens,
      animate && !reducedMotion && lcdMotion !== "off",
    );
  }, [animate, knownTokens, lcdMotion, ready, reducedMotion, text]);

  return (
    <div ref={rootRef} className="contents">
      <LazyMarkdownRenderer
        content={text}
        isDark={isDark}
        invertText={false}
        enableMermaid={enableMermaid}
        className="max-w-full break-words text-[var(--color-text-primary)] [overflow-wrap:anywhere]"
        fallback={fallback}
        onRendered={onRendered}
      />
    </div>
  );
}
