import type { CSSProperties, ReactNode } from "react";

import { cn } from "@/lib/utils";

export interface ShineBorderProps {
  className?: string;
  children?: ReactNode;
  borderWidth?: number;
  duration?: number;
  shineColor?: string | string[];
  topGlow?: boolean;
  style?: CSSProperties;
}

function normalizeShineColors(shineColor: ShineBorderProps["shineColor"]): string[] {
  if (Array.isArray(shineColor) && shineColor.length > 0) {
    return shineColor;
  }
  if (typeof shineColor === "string" && shineColor.trim()) {
    return [shineColor];
  }
  return ["#A07CFE", "#FE8FB5", "#FFBE7B"];
}

export function ShineBorder({
  className,
  children,
  borderWidth = 1,
  duration = 14,
  shineColor,
  topGlow = true,
  style,
}: ShineBorderProps) {
  const colors = normalizeShineColors(shineColor);
  const c0 = colors[0] || "#A07CFE";
  const c1 = colors[1] || c0;
  const c2 = colors[2] || c1;
  const ringStops = `conic-gradient(
    from 0deg,
    transparent 0deg 18deg,
    ${c0} 26deg,
    ${c1} 118deg,
    ${c2} 212deg,
    transparent 274deg 360deg
  )`;
  const trailStops = `conic-gradient(
    from 0deg,
    transparent 0deg 8deg,
    color-mix(in srgb, ${c0} 78%, white) 14deg,
    color-mix(in srgb, ${c1} 62%, white) 86deg,
    color-mix(in srgb, ${c2} 52%, white) 156deg,
    transparent 238deg 360deg
  )`;

  return (
    <div
      className={cn(
        "pointer-events-none absolute inset-0 overflow-hidden rounded-[inherit]",
        className,
      )}
      style={
        {
          ...style,
          padding: borderWidth,
          ["--shine-duration" as string]: `${duration}s`,
          ["--shine-border-bg" as string]: ringStops,
          ["--shine-border-trail" as string]: trailStops,
        } as CSSProperties
      }
    >
      <div
        className="absolute inset-0 rounded-[inherit]"
        style={{
          background: "var(--shine-border-bg)",
          animation:
            "shine-border-spin var(--shine-duration) cubic-bezier(0.55, 0.08, 0.35, 0.98) infinite",
          filter: "saturate(1.06) brightness(1.02)",
          transformOrigin: "center",
        }}
      />
      <div
        className="absolute inset-0 rounded-[inherit]"
        style={{
          background: "var(--shine-border-trail)",
          animation:
            "shine-border-drift calc(var(--shine-duration) * 0.72) cubic-bezier(0.4, 0, 0.2, 1) infinite",
          filter: "blur(1.2px) saturate(1.12)",
          opacity: 0.88,
          transformOrigin: "center",
          mixBlendMode: "screen",
        }}
      />
      {topGlow ? (
        <div
          className="absolute inset-0 rounded-[inherit]"
          style={{
            background:
              "radial-gradient(ellipse at 50% -10%, rgba(255,255,255,0.95) 0%, rgba(255,255,255,0.22) 16%, transparent 42%)",
            animation:
              "shine-border-top-glow calc(var(--shine-duration) * 0.9) ease-in-out infinite",
            mixBlendMode: "screen",
            opacity: 0.78,
          }}
        />
      ) : null}
      <div
        className="absolute rounded-[inherit] bg-transparent"
        style={{ inset: borderWidth, boxShadow: "inset 0 0 0 1px rgba(255,255,255,0.03)" }}
      />
      {children}
    </div>
  );
}
