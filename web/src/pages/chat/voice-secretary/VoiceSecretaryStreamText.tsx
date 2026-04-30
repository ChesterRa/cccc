import { useMemo } from "react";
import { classNames } from "../../../utils/classNames";

type StreamTextAnimateProps = {
  text: string;
  className?: string;
};

function splitStreamText(value: string): string[] {
  const text = String(value || "");
  if (!text) return [];
  const segmenterConstructor = typeof Intl !== "undefined"
    ? (Intl as typeof Intl & { Segmenter?: new (locale?: string, options?: { granularity?: "grapheme" }) => { segment: (input: string) => Iterable<{ segment: string }> } }).Segmenter
    : undefined;
  if (segmenterConstructor) {
    try {
      const segmenter = new segmenterConstructor(undefined, { granularity: "grapheme" });
      return Array.from(segmenter.segment(text), (item) => item.segment);
    } catch {
      // Fall back to Array.from below.
    }
  }
  return Array.from(text);
}

export function StreamTextAnimate({ text, className }: StreamTextAnimateProps) {
  const segments = useMemo(() => splitStreamText(text), [text]);
  if (!segments.length) return null;
  return (
    <span className={classNames("stream-text-animate", className || "")}>
      {segments.map((segment, index) => {
        if (!segment.trim()) return segment;
        return (
          <span
            key={`${index}-${segment}`}
            className="stream-text-animate-segment"
            style={{ animationDelay: `${Math.min(index, 36) * 12}ms` }}
          >
            {segment}
          </span>
        );
      })}
    </span>
  );
}
