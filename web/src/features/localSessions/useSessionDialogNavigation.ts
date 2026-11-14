import { useLayoutEffect, useRef, useState } from "react";

export type SessionDialogView = "sessions" | "group" | "device" | "help";

export function useSessionDialogNavigation() {
  const [view, setView] = useState<SessionDialogView>("sessions");
  const contentRef = useRef<HTMLDivElement>(null);
  const titleRef = useRef<HTMLHeadingElement>(null);
  const savedScroll = useRef(0);
  const firstRender = useRef(true);
  const navigate = (next: SessionDialogView) => {
    if (view === "sessions") savedScroll.current = contentRef.current?.scrollTop || 0;
    setView(next);
  };
  useLayoutEffect(() => {
    if (firstRender.current) {
      firstRender.current = false;
      return;
    }
    if (contentRef.current)
      contentRef.current.scrollTop = view === "sessions" ? savedScroll.current : 0;
    titleRef.current?.focus({ preventScroll: true });
  }, [view]);
  return { view, navigate, contentRef, titleRef };
}
