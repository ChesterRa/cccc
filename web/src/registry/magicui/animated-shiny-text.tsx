import type { ReactNode } from "react";

import { cn } from "@/lib/utils";

type AnimatedShinyTextProps = { className?: string; children: ReactNode };

export function AnimatedShinyText({ className, children }: AnimatedShinyTextProps) {
  return (
    <span
      className={cn(
        "animated-shiny-text inline-block bg-[length:200%_100%] bg-clip-text text-transparent",
        className,
      )}
    >
      {children}
    </span>
  );
}
