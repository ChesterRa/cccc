import type { CSSProperties } from "react";

import { cn } from "@/lib/utils";

type TextAnimateProps = {
  children: string;
  className?: string;
  segmentClassName?: string;
  animation?: "blurInUp";
  by?: "text" | "word" | "line";
  duration?: number;
  delay?: number;
  animate?: "all" | "append";
  stablePrefixLength?: number;
};

function splitText(value: string, by: NonNullable<TextAnimateProps["by"]>): string[] {
  if (by === "text") return [value];
  if (by === "line") return value.split(/(\n)/);
  return value.split(/(\s+)/);
}

export function TextAnimate({
  children,
  className,
  segmentClassName,
  animation = "blurInUp",
  by = "word",
  duration = 0.26,
  delay = 0.018,
  animate = "append",
  stablePrefixLength = 0,
}: TextAnimateProps) {
  const text = String(children || "");
  const prefixLength = animate === "append"
    ? Math.max(0, Math.min(text.length, Math.round(stablePrefixLength)))
    : 0;
  const stablePrefix = prefixLength > 0 ? text.slice(0, prefixLength) : "";
  const animatedText = stablePrefix ? text.slice(stablePrefix.length) : text;
  const parts = splitText(animatedText, by).map((part) => ({ part, itemIndex: -1 }));
  parts.reduce((count, item) => {
    if (item.part && item.part !== "\n" && !/^\s+$/.test(item.part)) {
      item.itemIndex = count;
      return count + 1;
    }
    return count;
  }, 0);
  return (
    <span
      className={cn("stream-text-animate", className)}
      aria-label={text}
    >
      {stablePrefix ? <span aria-hidden="true">{stablePrefix}</span> : null}
      {parts.map(({ part, itemIndex }, index) => {
        if (part === "\n") return <br key={`break-${index}`} />;
        if (!part) return null;
        if (/^\s+$/.test(part)) return <span key={`space-${index}`}>{part}</span>;
        return (
          <span
            key={`${part}-${index}`}
            aria-hidden="true"
            className={cn(
              animation === "blurInUp" && "stream-text-animate-segment",
              segmentClassName,
            )}
            style={{
              "--stream-text-duration": `${duration}s`,
              "--stream-text-delay": `${itemIndex * delay}s`,
            } as CSSProperties}
          >
            {part}
          </span>
        );
      })}
    </span>
  );
}
