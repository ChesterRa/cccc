"use client";

// Adapted from Magic UI Highlighter: https://magicui.design/docs/components/highlighter
import { useLayoutEffect, useRef } from "react";
import type React from "react";
import { useInView } from "motion/react";
import { cn } from "@/lib/utils";
import { createHighlighterAnnotation } from "./highlighterAnnotation";

type AnnotationAction =
  | "highlight"
  | "underline"
  | "box"
  | "circle"
  | "strike-through"
  | "crossed-off"
  | "bracket";

interface HighlighterProps extends React.HTMLAttributes<HTMLSpanElement> {
  children: React.ReactNode;
  action?: AnnotationAction;
  color?: string;
  strokeWidth?: number;
  animationDuration?: number;
  iterations?: number;
  padding?: number;
  multiline?: boolean;
  isView?: boolean;
  animate?: boolean;
  className?: string;
}

export function Highlighter({
  children,
  action = "highlight",
  color = "#ffd1dc",
  strokeWidth = 1.5,
  animationDuration = 600,
  iterations = 2,
  padding = 2,
  multiline = true,
  isView = false,
  animate = true,
  className,
  ...spanProps
}: HighlighterProps) {
  const elementRef = useRef<HTMLSpanElement>(null);

  const isInView = useInView(elementRef, { once: true, margin: "-10%" });

  // If isView is false, always show. If isView is true, wait for inView
  const shouldShow = !isView || isInView;

  useLayoutEffect(() => {
    const element = elementRef.current;
    if (!shouldShow || !element) return;
    let resizeObserver: ResizeObserver | null = null;
    const annotation = createHighlighterAnnotation(element, {
      type: action,
      color,
      strokeWidth,
      animationDuration,
      iterations,
      padding,
      multiline,
      animate,
    });

    if (typeof ResizeObserver !== "undefined") {
      resizeObserver = new ResizeObserver(() => {
        annotation.hide();
        annotation.show();
      });

      resizeObserver.observe(element);
      if (element.parentElement) resizeObserver.observe(element.parentElement);
      resizeObserver.observe(document.body);
    }

    return () => {
      annotation.remove();
      resizeObserver?.disconnect();
    };
  }, [
    shouldShow,
    action,
    color,
    strokeWidth,
    animationDuration,
    iterations,
    padding,
    multiline,
    animate,
  ]);

  return (
    <span
      ref={elementRef}
      className={cn("relative inline-block bg-transparent", className)}
      {...spanProps}
    >
      {children}
    </span>
  );
}
