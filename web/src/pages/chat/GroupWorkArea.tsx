import { useEffect, useRef, useState, type ReactNode } from "react";
import { createPortal } from "react-dom";
import { useTranslation } from "react-i18next";
import { ChevronLeft, ChevronRight, MessageSquare, LayoutGrid } from "lucide-react";
import type { Actor } from "../../types";
import { useUIStore } from "../../stores";
import { getChatSession } from "../../stores/useUIStore";
import { RuntimeInspectorModal } from "../../components/modals/RuntimeInspectorModal";
import { ErrorBoundary } from "../../components/ErrorBoundary";
import { terminalPageLayout } from "./groupWorkLayout";

export type RuntimeActorView = {
  isVisible: boolean;
  compact: boolean;
  onExpand: () => void;
  navigation?: ReactNode;
};

type Props = {
  groupId: string;
  actors: Actor[];
  renderedActorIds: string[];
  activeActorId?: string;
  isDark: boolean;
  isVisible: boolean;
  loading: boolean;
  workControlsHost: HTMLElement | null;
  isSmallScreen: boolean;
  headerEnd?: ReactNode;
  onInspectActor: (actorId: string) => void;
  renderActor: (actorId: string, view: RuntimeActorView) => ReactNode;
  children: ReactNode;
};

export function GroupWorkArea({
  groupId,
  actors,
  renderedActorIds,
  activeActorId,
  isDark,
  isVisible,
  loading,
  workControlsHost,
  isSmallScreen,
  headerEnd,
  onInspectActor,
  renderActor,
  children,
}: Props) {
  const { t } = useTranslation("chat");
  const session = useUIStore((state) => getChatSession(groupId, state.chatSessions));
  const setView = useUIStore((state) => state.setGroupWorkView);
  const setPage = useUIStore((state) => state.setGroupTerminalPage);
  const root = useRef<HTMLDivElement>(null);
  const [width, setWidth] = useState(0);
  const [visited, setVisited] = useState(false);
  const tiled = session.workView === "terminals";
  const { page, pageCount, pageSize, start } = terminalPageLayout(
    actors.length,
    session.terminalPage,
    width,
  );
  const pageActors = actors.slice(start, start + pageSize);
  const pageIds = new Set(pageActors.map((actor) => actor.id));
  // Retain this page across view changes and preserve the same instance on maximize.
  // Paging disposes unseen tiles; the Actor and its input remain owned by the daemon.
  const ids = [
    ...new Set([
      ...(tiled || visited ? pageActors.map((actor) => actor.id) : []),
      ...renderedActorIds,
      ...(activeActorId ? [activeActorId] : []),
    ]),
  ];

  useEffect(() => {
    if (!tiled) return;
    const timer = window.setTimeout(() => setVisited(true), 0);
    return () => window.clearTimeout(timer);
  }, [tiled]);

  const layoutRef = useRef({ actors, pageSize, start, loading });
  const measuredWidth = useRef(0);
  useEffect(() => {
    layoutRef.current = { actors, pageSize, start, loading };
  }, [actors, pageSize, start, loading]);

  useEffect(() => {
    const element = root.current;
    if (!element) return;
    const update = () => {
      const nextWidth = element.clientWidth;
      if (nextWidth <= 0) return;
      const previous = layoutRef.current;
      const nextSize = terminalPageLayout(previous.actors.length, 0, nextWidth).pageSize;
      if (
        measuredWidth.current > 0 &&
        nextSize !== previous.pageSize &&
        !previous.loading &&
        previous.actors.length
      ) {
        const focused = document.activeElement?.closest<HTMLElement>("[data-runtime-actor-id]");
        const index =
          focused && element.contains(focused)
            ? previous.actors.findIndex((actor) => actor.id === focused.dataset.runtimeActorId)
            : -1;
        setPage(groupId, Math.floor((index >= 0 ? index : previous.start) / nextSize));
      }
      measuredWidth.current = nextWidth;
      setWidth(nextWidth);
    };
    update();
    const observer = new ResizeObserver(update);
    observer.observe(element);
    return () => observer.disconnect();
  }, [groupId, setPage]);

  const focusActor = (actorId: string) => {
    requestAnimationFrame(() => {
      const pane = root.current?.querySelector<HTMLElement>(
        `[data-runtime-actor-id="${CSS.escape(actorId)}"]`,
      );
      (pane?.querySelector<HTMLElement>(".xterm-helper-textarea") || pane)?.focus();
    });
  };
  const chooseActor = (actorId: string) => {
    const index = actors.findIndex((actor) => actor.id === actorId);
    if (index < 0) return;
    setPage(groupId, Math.floor(index / pageSize));
    focusActor(actorId);
  };
  const otherAttention = actors.filter(
    (actor) =>
      !pageIds.has(actor.id) &&
      (actor.effective_working_state === "waiting" || actor.effective_working_state === "stuck"),
  );
  const buttonClass =
    "inline-flex h-8 shrink-0 items-center justify-center rounded-md px-2 text-xs hover:bg-[var(--glass-tab-bg)] disabled:opacity-35 focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-[var(--color-text-secondary)]";

  const pager =
    pageCount > 1 && tiled ? (
      <nav className="flex shrink-0 items-center text-xs" aria-label={t("workView.pages")}>
        <button
          type="button"
          className={buttonClass}
          disabled={page === 0 || loading}
          onClick={() => setPage(groupId, page - 1)}
          aria-label={t("workView.previous")}
        >
          <ChevronLeft size={14} />
        </button>
        <span className="tabular-nums text-[var(--color-text-secondary)]">
          {page + 1}/{pageCount}
        </span>
        <button
          type="button"
          className={buttonClass}
          disabled={page + 1 === pageCount || loading}
          onClick={() => setPage(groupId, page + 1)}
          aria-label={t("workView.next")}
        >
          <ChevronRight size={14} />
        </button>
        {otherAttention.length > 0 ? (
          <button
            type="button"
            className={`${buttonClass} text-amber-700 dark:text-amber-300`}
            onClick={() => chooseActor(otherAttention[0].id)}
            aria-label={t("workView.otherAttention", { count: otherAttention.length })}
            title={t("workView.otherAttention", { count: otherAttention.length })}
          >
            !
          </button>
        ) : null}
      </nav>
    ) : null;

  const unread =
    session.chatUnreadCount > 0 ? (
      <span
        className="ml-1 text-[10px] tabular-nums"
        aria-label={t("workView.unread", { count: session.chatUnreadCount })}
      >
        {session.chatUnreadCount > 99 ? "99+" : session.chatUnreadCount}
      </span>
    ) : null;
  return (
    <div ref={root} className="relative flex min-h-0 min-w-0 flex-1 flex-col" data-group-work-area>
      {workControlsHost
        ? createPortal(
            <>
              <div
                className="hidden shrink-0 items-center rounded-lg bg-[var(--glass-tab-bg)] p-0.5 @min-[600px]/group-header:inline-flex"
                role="group"
                aria-label={t("workView.label")}
                data-group-view-switch
              >
                {(["messages", "terminals"] as const).map((view) => (
                  <button
                    key={view}
                    type="button"
                    aria-pressed={session.workView === view}
                    className={`${buttonClass} ${session.workView === view ? "bg-[var(--color-bg-primary)] font-semibold text-[var(--color-text-primary)]" : "text-[var(--color-text-tertiary)]"}`}
                    onClick={() => setView(groupId, view)}
                  >
                    {t(`workView.${view}`)}
                    {view === "messages" ? unread : null}
                  </button>
                ))}
              </div>
              <button
                type="button"
                className={`${buttonClass} @min-[600px]/group-header:hidden`}
                data-group-view-toggle
                aria-label={t("workView.switchTo", {
                  view: t(tiled ? "workView.messages" : "workView.terminals"),
                })}
                title={t("workView.switchTo", {
                  view: t(tiled ? "workView.messages" : "workView.terminals"),
                })}
                onClick={() => setView(groupId, tiled ? "messages" : "terminals")}
              >
                {tiled ? <LayoutGrid size={17} /> : <MessageSquare size={17} />}
                {unread}
              </button>
              {!isSmallScreen ? pager : null}
              {headerEnd}
            </>,
            workControlsHost,
          )
        : null}
      <div
        className={tiled ? "hidden" : "relative flex min-h-0 flex-1 flex-col"}
        inert={tiled ? true : undefined}
        aria-hidden={tiled ? true : undefined}
        data-group-message-view
      >
        {children}
      </div>
      <div
        className={tiled ? "grid min-h-0 min-w-0 flex-1 gap-2 p-2" : "contents"}
        style={
          tiled
            ? {
                gridTemplateColumns:
                  pageSize === 1 || pageActors.length < 2
                    ? "minmax(0,1fr)"
                    : "repeat(2,minmax(0,1fr))",
                gridTemplateRows:
                  pageActors.length > 2 ? "repeat(2,minmax(0,1fr))" : "minmax(0,1fr)",
              }
            : undefined
        }
        data-group-terminal-view={tiled ? "visible" : "hidden"}
      >
        {ids.map((actorId) => {
          const expanded = activeActorId === actorId;
          const inPage = tiled && pageIds.has(actorId);
          const visible = isVisible && (inPage || expanded);
          return (
            <div
              key={actorId}
              className={
                inPage
                  ? "min-h-0 min-w-0 overflow-hidden rounded-lg border border-[var(--glass-border-subtle)] focus-within:border-[var(--color-text-secondary)] focus-within:ring-1 focus-within:ring-[var(--color-text-secondary)]"
                  : expanded
                    ? "contents"
                    : "hidden"
              }
              tabIndex={-1}
              data-runtime-actor-id={actorId}
            >
              <RuntimeInspectorModal
                isOpen={visible}
                inline={!expanded}
                isDark={isDark}
                onClose={() => {
                  onInspectActor("chat");
                  if (inPage) focusActor(actorId);
                }}
                titleId={`runtime-inspector-${actorId}`}
                closeAriaLabel={t("workView.restore")}
              >
                <ErrorBoundary>
                  {renderActor(actorId, {
                    isVisible: visible,
                    compact: !expanded,
                    navigation: inPage && !expanded && isSmallScreen ? pager : undefined,
                    onExpand: () => onInspectActor(actorId),
                  })}
                </ErrorBoundary>
              </RuntimeInspectorModal>
            </div>
          );
        })}
        {tiled && actors.length === 0 ? (
          <div className="flex min-h-0 items-center justify-center p-6 text-center text-sm text-[var(--color-text-tertiary)]">
            {t(loading ? "workView.loading" : "workView.empty")}
          </div>
        ) : null}
      </div>
    </div>
  );
}
