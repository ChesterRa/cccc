import { useCallback, useEffect, useLayoutEffect, useRef, type RefObject } from "react";
import type { WorkspaceFileNavigation } from "../workspace/useWorkspaceEditor";

/** The element that scrolls a workspace file: its source editor, plain text, Markdown or table. */
export const WORKSPACE_SCROLLER = "[data-workspace-scroll]";
/** How long a restore waits for lazily rendered content (Markdown, diagrams) to reach its height. */
const RESTORE_WINDOW_MS = 2000;
const USER_INPUT = ["wheel", "touchstart", "pointerdown", "keydown"] as const;

/**
 * Remembers where each workspace tab was read, since every workspace tab shares one viewer.
 *
 * `shownKey` is the tab whose own file is on screen and loaded, or "" while another file is still
 * showing or loading, so a position is never recorded for, or carried over to, the wrong file. A
 * file just opened at an anchor keeps the anchor's position; the editor keeps an applied anchor
 * around, so only an anchor this tab has not been shown with yet counts as new.
 */
export function useWorkspaceScrollMemory(
  body: RefObject<HTMLElement | null>,
  shownKey: string,
  navigation: WorkspaceFileNavigation | null,
) {
  const positions = useRef(new Map<string, number>());
  const anchors = useRef(new Map<string, WorkspaceFileNavigation>());
  const recording = useRef("");

  useEffect(() => {
    const node = body.current;
    if (!node) return undefined;
    const onScroll = (event: Event) => {
      const target = event.target;
      if (!(target instanceof HTMLElement) || !target.matches(WORKSPACE_SCROLLER)) return;
      if (recording.current) positions.current.set(recording.current, target.scrollTop);
    };
    node.addEventListener("scroll", onScroll, true);
    return () => node.removeEventListener("scroll", onScroll, true);
  }, [body]);

  useLayoutEffect(() => {
    recording.current = "";
    const node = body.current;
    if (!shownKey || !node) return undefined;
    if (navigation && anchors.current.get(shownKey) !== navigation) {
      anchors.current.set(shownKey, navigation);
      recording.current = shownKey;
      return undefined;
    }
    const saved = positions.current.get(shownKey) ?? 0;
    if (saved === 0) {
      // Nothing to wait for: a file not read yet starts at the top, even in a reused scroller.
      const scroller = node.querySelector<HTMLElement>(WORKSPACE_SCROLLER);
      if (scroller) scroller.scrollTop = 0;
      recording.current = shownKey;
      return undefined;
    }
    let frame = 0;
    let done = false;
    const started = performance.now();
    const finish = () => {
      if (done) return;
      done = true;
      cancelAnimationFrame(frame);
      for (const type of USER_INPUT) node.removeEventListener(type, finish, true);
      recording.current = shownKey;
    };
    // Lazily rendered content grows over a few frames; the position applies once it fits.
    const apply = () => {
      const scroller = node.querySelector<HTMLElement>(WORKSPACE_SCROLLER);
      if (scroller) {
        scroller.scrollTop = saved;
        if (Math.abs(scroller.scrollTop - saved) <= 1) return finish();
      }
      if (performance.now() - started >= RESTORE_WINDOW_MS) return finish();
      frame = requestAnimationFrame(apply);
    };
    // Reading or scrolling during the wait means the reader has moved on; keep their position.
    for (const type of USER_INPUT) node.addEventListener(type, finish, true);
    apply();
    return finish;
  }, [body, navigation, shownKey]);

  /** A closed tab's position goes with it. */
  const forget = useCallback((keys: string[]) => {
    for (const key of keys) {
      positions.current.delete(key);
      anchors.current.delete(key);
    }
  }, []);
  return forget;
}
