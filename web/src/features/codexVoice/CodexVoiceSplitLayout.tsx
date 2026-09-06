import { useEffect, useRef, useState, type ReactNode } from "react";
import { useTranslation } from "react-i18next";

const STORAGE_KEY = "cccc.codexVoice.splitRatio.v1";
const DEFAULT_RATIO = (100 * 1.15) / 2.15;
const HANDLE_WIDTH = 8;

function loadRatio(): number {
  try {
    const value = Number(window.localStorage.getItem(STORAGE_KEY));
    if (Number.isFinite(value) && value > 0 && value < 100) return value;
  } catch {
    // Layout preferences are optional when browser storage is unavailable.
  }
  return DEFAULT_RATIO;
}

export function CodexVoiceSplitLayout({
  enabled,
  active,
  conversation,
  analyst,
}: {
  enabled: boolean;
  active: boolean;
  conversation: ReactNode;
  analyst: ReactNode;
}) {
  const { t } = useTranslation("modals");
  const container = useRef<HTMLDivElement>(null);
  const handle = useRef<HTMLDivElement>(null);
  const drag = useRef<{ pointerId: number; original: number } | null>(null);
  const [ratio, setRatio] = useState(loadRatio);
  const currentRatio = useRef(ratio);
  const [width, setWidth] = useState(1180);
  const available = Math.max(1, width - HANDLE_WIDTH);
  const minimum = Math.min(50, (300 / available) * 100);
  const maximum = Math.max(50, 100 - (360 / available) * 100);
  const clamp = (value: number) => Math.min(maximum, Math.max(minimum, value));
  const displayedRatio = clamp(ratio);

  const updateRatio = (value: number, persist = false) => {
    currentRatio.current = value;
    setRatio(value);
    if (persist) {
      try {
        window.localStorage.setItem(STORAGE_KEY, String(value));
      } catch {
        // Keep resizing usable even if this browser cannot remember it.
      }
    }
  };

  useEffect(() => {
    const element = container.current;
    if (!element || !enabled || !active) return;
    const measure = () => {
      // Opening the modal animates its transform; pane limits use layout width.
      const measured = element.clientWidth;
      if (measured > HANDLE_WIDTH) setWidth(measured);
    };
    measure();
    const observer = new ResizeObserver(measure);
    observer.observe(element);
    return () => observer.disconnect();
  }, [enabled, active]);

  useEffect(() => {
    if (enabled && active) return;
    const previous = drag.current;
    if (!previous) return;
    drag.current = null;
    currentRatio.current = previous.original;
    setRatio(previous.original);
    if (handle.current?.hasPointerCapture(previous.pointerId)) {
      handle.current.releasePointerCapture(previous.pointerId);
    }
  }, [enabled, active]);

  return (
    <div
      ref={container}
      className="grid min-h-0 min-w-0 flex-1"
      style={
        enabled
          ? {
              gridTemplateColumns: `minmax(0,${displayedRatio}fr) ${HANDLE_WIDTH}px minmax(0,${100 - displayedRatio}fr)`,
            }
          : undefined
      }
    >
      {conversation}
      <div
        ref={handle}
        role="separator"
        tabIndex={enabled && active ? 0 : -1}
        aria-label={t("codexVoiceResizePanes")}
        aria-controls="codex-voice-conversation-pane codex-voice-analyst-pane"
        aria-orientation="vertical"
        aria-valuemin={Math.round(minimum)}
        aria-valuemax={Math.round(maximum)}
        aria-valuenow={Math.round(displayedRatio)}
        aria-valuetext={t("codexVoicePaneRatio", {
          conversation: Math.round(displayedRatio),
          analyst: 100 - Math.round(displayedRatio),
        })}
        title={t("codexVoiceResizePanesHint")}
        hidden={!enabled}
        className={`${enabled ? "flex" : "hidden"} group touch-none select-none cursor-col-resize items-center justify-center bg-[var(--glass-panel-bg)] outline-none hover:bg-[var(--glass-tab-bg-active)] focus-visible:ring-2 focus-visible:ring-inset focus-visible:ring-[var(--color-accent-primary)]`}
        onPointerDown={(event) => {
          if (event.button !== 0 || !enabled || !active || drag.current) return;
          event.preventDefault();
          event.currentTarget.focus();
          event.currentTarget.setPointerCapture(event.pointerId);
          drag.current = { pointerId: event.pointerId, original: currentRatio.current };
        }}
        onPointerMove={(event) => {
          if (drag.current?.pointerId !== event.pointerId) return;
          const bounds = container.current?.getBoundingClientRect();
          if (!bounds || bounds.width <= HANDLE_WIDTH) return;
          updateRatio(
            clamp(
              ((event.clientX - bounds.left - HANDLE_WIDTH / 2) / (bounds.width - HANDLE_WIDTH)) *
                100,
            ),
          );
        }}
        onPointerUp={(event) => {
          if (drag.current?.pointerId !== event.pointerId) return;
          drag.current = null;
          updateRatio(currentRatio.current, true);
          event.currentTarget.releasePointerCapture(event.pointerId);
        }}
        onLostPointerCapture={() => {
          if (!drag.current) return;
          const original = drag.current.original;
          drag.current = null;
          updateRatio(original);
        }}
        onPointerCancel={(event) => {
          if (drag.current?.pointerId === event.pointerId) {
            const original = drag.current.original;
            drag.current = null;
            updateRatio(original);
          }
        }}
        onDoubleClick={() => updateRatio(DEFAULT_RATIO, true)}
        onKeyDown={(event) => {
          const step = event.shiftKey ? 10 : 2;
          const next = {
            ArrowLeft: displayedRatio - step,
            ArrowRight: displayedRatio + step,
            Home: minimum,
            End: maximum,
          }[event.key];
          if (next === undefined) return;
          event.preventDefault();
          updateRatio(clamp(next), true);
        }}
      >
        <span
          aria-hidden="true"
          className="h-10 w-0.5 rounded-full bg-[var(--glass-border-subtle)] group-hover:bg-[var(--color-text-muted)] group-focus-visible:bg-[var(--color-accent-primary)]"
        />
      </div>
      {analyst}
    </div>
  );
}
