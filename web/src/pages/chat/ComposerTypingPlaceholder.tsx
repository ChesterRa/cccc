import { useEffect, useLayoutEffect, useMemo, useRef, useState, useSyncExternalStore } from "react";
import { useTranslation } from "react-i18next";

const REDUCED_MOTION = "(prefers-reduced-motion: reduce)";

function subscribeMotionState(onChange: () => void) {
  const media = window.matchMedia(REDUCED_MOTION);
  media.addEventListener("change", onChange);
  document.addEventListener("visibilitychange", onChange);
  return () => {
    media.removeEventListener("change", onChange);
    document.removeEventListener("visibilitychange", onChange);
  };
}

function getMotionState() {
  if (window.matchMedia(REDUCED_MOTION).matches) return "reduced";
  return document.visibilityState === "hidden" ? "hidden" : "running";
}

function TypingText({ words, focused }: { words: string[]; focused: boolean }) {
  const motion = useSyncExternalStore(subscribeMotionState, getMotionState, () => "reduced");
  const [frame, setFrame] = useState({ index: 0, count: 0, deleting: false });
  const characters = Array.from(words[frame.index]);
  const playing = !focused && motion === "running";

  useEffect(() => {
    if (!playing) return;
    const complete = frame.count === characters.length;
    const delay = frame.deleting ? 30 : complete ? 3000 : 60;
    const timer = window.setTimeout(() => {
      setFrame((current) => {
        if (current.deleting) {
          return current.count > 0
            ? { ...current, count: current.count - 1 }
            : { index: (current.index + 1) % words.length, count: 0, deleting: false };
        }
        return complete ? { ...current, deleting: true } : { ...current, count: current.count + 1 };
      });
    }, delay);
    return () => window.clearTimeout(timer);
  }, [playing, frame, characters.length, words.length]);

  const text = motion === "reduced" ? words[0] : characters.slice(0, frame.count).join("");
  return (
    <>
      <span data-composer-typing-text>{text || (focused ? words[0] : "")}</span>
      {playing && (
        <span className="ml-px inline-block h-[1em] border-r border-current align-[-0.12em] motion-safe:animate-pulse" />
      )}
    </>
  );
}

/** Decorative text only: never becomes input value or an announced live region. */
export function ComposerTypingPlaceholder({
  focused,
  fontSize,
  lineHeight,
  onHeightChange,
}: {
  focused: boolean;
  fontSize: number;
  lineHeight: number;
  onHeightChange?: (height: number) => void;
}) {
  const { t } = useTranslation("chat");
  const words = useMemo(
    () => [
      t("composerPromptTask"),
      t("composerPromptSkill"),
      t("composerPromptActor"),
      t("composerPromptGroup"),
    ],
    [t],
  );
  const measurement = useRef<HTMLDivElement>(null);
  useLayoutEffect(() => {
    const element = measurement.current;
    if (!element || !onHeightChange) return;
    const measure = () => onHeightChange(Math.ceil(element.getBoundingClientRect().height));
    measure();
    const observer = new ResizeObserver(measure);
    observer.observe(element);
    return () => observer.disconnect();
  }, [words, fontSize, lineHeight, onHeightChange]);

  return (
    <div
      data-composer-typing-placeholder
      aria-hidden="true"
      className="pointer-events-none absolute inset-0 overflow-hidden whitespace-pre-wrap break-words text-[var(--color-text-muted)]"
      style={{ fontSize, lineHeight: `${lineHeight}px` }}
    >
      <div ref={measurement} className="invisible grid px-4 py-3">
        {words.map((word) => (
          <span key={word} className="col-start-1 row-start-1">
            {word}
          </span>
        ))}
      </div>
      <div className="absolute inset-x-0 top-0 px-4 py-3">
        <TypingText key={words.join("\u0000")} words={words} focused={focused} />
      </div>
    </div>
  );
}
