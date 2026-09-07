import type { ReactNode } from "react";
import { createPortal } from "react-dom";
import "./voiceMobileControls.css";

/** Keep mobile status out of the fixed-width action bar without duplicating state. */
export function VoiceComposerStatus({
  target,
  children,
}: {
  target?: HTMLElement | null;
  children: ReactNode;
}) {
  return (
    <>
      <div className="voice-desktop-status">{children}</div>
      {target ? createPortal(<div className="voice-mobile-status">{children}</div>, target) : null}
    </>
  );
}
