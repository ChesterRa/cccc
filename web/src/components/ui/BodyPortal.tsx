import { useEffect, useMemo } from "react";
import { createPortal } from "react-dom";
import type { ReactNode } from "react";

interface BodyPortalProps {
  children: ReactNode;
  className?: string;
  /** Takes the LCD style when it is on, like ModalFrame's lcdScope. */
  lcdScope?: boolean;
}

export function BodyPortal({ children, className = "", lcdScope = false }: BodyPortalProps) {
  const host = useMemo(() => {
    if (typeof document === "undefined") return null;
    const node = document.createElement("div");
    if (className) node.className = className;
    if (lcdScope) node.setAttribute("data-lcd-scope", "");
    return node;
  }, [className, lcdScope]);

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
