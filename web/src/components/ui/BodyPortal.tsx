import { useEffect, useMemo } from "react";
import { createPortal } from "react-dom";
import type { ReactNode } from "react";

interface BodyPortalProps {
  children: ReactNode;
  className?: string;
}

export function BodyPortal({ children, className = "" }: BodyPortalProps) {
  const host = useMemo(() => {
    if (typeof document === "undefined") return null;
    const node = document.createElement("div");
    if (className) node.className = className;
    return node;
  }, [className]);

  useEffect(() => {
    if (!host) return undefined;
    document.body.appendChild(host);
    return () => {
      if (host.parentNode) {
        host.parentNode.removeChild(host);
      }
    };
  }, [host]);

  if (!host) return null;
  return createPortal(children, host);
}
