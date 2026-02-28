import { useEffect, useRef, useState } from "react";
import { classNames } from "../utils/classNames";

interface MermaidDiagramProps {
  chart: string;
  isDark: boolean;
  className?: string;
  fitMode?: "contain" | "natural";
}

export function MermaidDiagram({ chart, isDark, className, fitMode = "contain" }: MermaidDiagramProps) {
  const containerRef = useRef<HTMLDivElement>(null);
  const [error, setError] = useState("");

  useEffect(() => {
    let cancelled = false;
    const target = containerRef.current;
    if (!target) return;
    target.innerHTML = "";
    setError("");

    const source = String(chart || "").trim();
    if (!source) return;

    const run = async () => {
      try {
        const mod = await import("mermaid");
        const mermaid = mod.default;
        mermaid.initialize({
          startOnLoad: false,
          securityLevel: "strict",
          theme: isDark ? "dark" : "default",
          fontFamily: "inherit",
          flowchart: { useMaxWidth: false },
        });
        const id = `cccc-mermaid-${Date.now()}-${Math.random().toString(36).slice(2, 10)}`;
        const rendered = await mermaid.render(id, source);
        if (cancelled || !containerRef.current) return;
        containerRef.current.innerHTML = rendered.svg;
        const svg = containerRef.current.querySelector("svg");
        if (svg) {
          svg.style.maxWidth = fitMode === "contain" ? "100%" : "none";
          svg.style.height = "auto";
          svg.style.width = fitMode === "contain" ? "100%" : "max-content";
          svg.style.minWidth = fitMode === "contain" ? "0" : "max(100%, 960px)";
          svg.style.fontSize = fitMode === "contain" ? "13px" : "14px";
        }
        rendered.bindFunctions?.(containerRef.current);
      } catch (e) {
        if (cancelled) return;
        setError(e instanceof Error ? e.message : "Failed to render mermaid");
      }
    };
    void run();

    return () => {
      cancelled = true;
    };
  }, [chart, isDark, fitMode]);

  if (error) {
    return (
      <div className={classNames("space-y-2", className)}>
        <div className={classNames("text-[11px]", isDark ? "text-rose-400" : "text-rose-600")}>
          Mermaid render failed: {error}
        </div>
        <pre
          className={classNames(
            "text-[11px] rounded-lg p-2 overflow-auto",
            isDark ? "bg-slate-900 text-slate-300" : "bg-white text-gray-700 border border-gray-200"
          )}
        >
          {chart}
        </pre>
      </div>
    );
  }

  return (
    <div
      ref={containerRef}
      className={classNames(fitMode === "contain" ? "overflow-hidden" : "overflow-auto", "[&_svg]:select-none", className)}
    />
  );
}
