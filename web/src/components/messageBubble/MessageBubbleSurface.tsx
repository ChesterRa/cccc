import type { ReactNode } from "react";

import { classNames } from "../../utils/classNames";
import { Surface } from "../ui/surface";

interface MessageBubbleSurfaceProps {
  children: ReactNode;
  isUserMessage: boolean;
  isStreaming: boolean;
  motionClass: string;
  isAttention: boolean;
  isHighlighted: boolean;
}

function sharedClasses({
  isStreaming,
  motionClass,
  isAttention,
  isHighlighted,
}: Omit<MessageBubbleSurfaceProps, "children" | "isUserMessage">): string {
  return classNames(
    "inline-flex max-w-full min-w-0 flex-col px-4 py-3 text-sm leading-relaxed",
    "transition-[opacity,transform,box-shadow,background-color,border-color] duration-200 ease-out",
    isStreaming ? "translate-y-0 opacity-95" : "translate-y-0 opacity-100",
    motionClass,
    isAttention ? "ring-1 ring-amber-400/40 dark:ring-amber-500/40" : "",
    isHighlighted ? "outline outline-2 outline-[var(--glass-accent-border)] outline-offset-2" : "",
  );
}

export function MessageBubbleSurface({
  children,
  isUserMessage,
  isStreaming,
  motionClass,
  isAttention,
  isHighlighted,
}: MessageBubbleSurfaceProps) {
  const className = sharedClasses({ isStreaming, motionClass, isAttention, isHighlighted });

  if (isUserMessage) {
    return (
      <div
        className={classNames(
          className,
          "glass-bubble w-auto min-w-[min(18rem,70vw)] rounded-[22px] rounded-tr-md",
        )}
      >
        {children}
      </div>
    );
  }

  return (
    <Surface
      variant="subtle"
      padding="none"
      radius="lg"
      className={classNames(
        className,
        "w-full text-[var(--color-text-primary)] shadow-[var(--glass-bubble-shadow)]",
      )}
    >
      {children}
    </Surface>
  );
}
