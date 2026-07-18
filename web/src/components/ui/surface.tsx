import * as React from "react";
import { cva, type VariantProps } from "class-variance-authority";

import { cn } from "@/lib/utils";

const surfaceVariants = cva("border", {
  variants: {
    variant: {
      default: "glass-panel border-[var(--glass-panel-border)]",
      subtle: "border-[var(--glass-border-subtle)] bg-[var(--glass-panel-bg)]",
      elevated: "glass-modal border-[var(--glass-border-subtle)]",
    },
    padding: { none: "", sm: "p-3", md: "p-4", lg: "p-5 sm:p-6" },
    radius: { md: "rounded-xl", lg: "rounded-2xl", xl: "rounded-3xl" },
  },
  defaultVariants: { variant: "default", padding: "md", radius: "lg" },
});

export interface SurfaceProps
  extends React.HTMLAttributes<HTMLDivElement>, VariantProps<typeof surfaceVariants> {}

function Surface({ className, variant, padding, radius, ...props }: SurfaceProps) {
  return (
    <div className={cn(surfaceVariants({ variant, padding, radius }), className)} {...props} />
  );
}

export { Surface };
