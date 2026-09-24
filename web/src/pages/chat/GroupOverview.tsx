import {
  memo,
  useEffect,
  useLayoutEffect,
  useMemo,
  useRef,
  useState,
  type CSSProperties,
} from "react";
import { useTranslation } from "react-i18next";
import { Mail } from "lucide-react";

import { ActorAvatar } from "../../components/ActorAvatar";
import { useActorDisplayState } from "../../hooks/useActorDisplayState";
import { fetchContext } from "../../services/api/context";
import { fetchTerminalTail } from "../../services/api/diagnostics";
import { fetchLedgerTailAll } from "../../services/api/messaging";
import { selectChatBucketState, useGroupStore } from "../../stores";
import type { Actor, AgentState, ChatMessageData, LedgerEvent } from "../../types";
import { classNames } from "../../utils/classNames";
import { getRecipientActorIdsForEvent, isChatMessageEvent } from "../../utils/ledgerEventHandlers";
import { formatElapsedCompact } from "../../utils/time";
import {
  buildActorStoppedSinceMap,
  buildRuntimeDockMailInfo,
  type RuntimeDockMailInfo,
} from "./runtimeDockMail";
import { useElapsedNow, useStateSinceMs } from "./runtimeDockElapsed";

type Props = {
  groupId: string;
  actors: Actor[];
  isDark: boolean;
  loading: boolean;
  actorStatusProvisional?: boolean;
  onOpenActor: (actorId: string) => void;
};

type OverviewTone = "stopped" | "idle" | "working" | "waiting" | "stuck" | "running";

function toneFor(isRunning: boolean, workingState: string): OverviewTone {
  if (!isRunning) return "stopped";
  const state = String(workingState || "")
    .trim()
    .toLowerCase();
  if (state === "working" || state === "waiting" || state === "stuck") return state;
  if (state === "idle" || state === "") return "idle";
  return "running";
}

const TONE_CLASSES: Record<OverviewTone, { ring: string; labelKey: string }> = {
  stopped: { ring: "ring-2 ring-slate-400/50", labelKey: "stopped" },
  idle: { ring: "ring-2 ring-emerald-500/75", labelKey: "idle" },
  running: { ring: "ring-2 ring-emerald-500/75", labelKey: "running" },
  working: { ring: "ring-2 ring-violet-400/85", labelKey: "working" },
  waiting: { ring: "ring-2 ring-amber-400/85", labelKey: "waiting" },
  stuck: { ring: "ring-2 ring-rose-500/85", labelKey: "stuck" },
};

const TONE_PILL_CLASSES: Record<OverviewTone, string> = {
  stopped: "bg-slate-600/60 text-slate-200 ring-slate-400/30",
  idle: "bg-emerald-500/15 text-emerald-300 ring-emerald-500/30",
  running: "bg-emerald-500/15 text-emerald-300 ring-emerald-500/30",
  working: "bg-white/95 text-black ring-black/10",
  waiting: "bg-amber-500/15 text-amber-300 ring-amber-500/30",
  stuck: "bg-rose-500/20 text-rose-200 ring-rose-400/40",
};

const GRID_SLOTS = [0, 1, 2, 5, 8, 7, 6, 3]; // actor index -> cell; cell 4 = foreman
const PAGE_SIZE = 8;
const MAX_EDGES = 8;
const TERMINAL_POLL_MS = 4000;
const AGENT_STATE_POLL_MS = 20000;

type ExcerptMode = "msg" | "tty";

type CellRect = { l: number; r: number; t: number; b: number; cx: number; cy: number };

type DirectEdge = { fromId: string; toId: string; label: string; isMail: boolean; rank: number };

function getMessagePreview(event: LedgerEvent | undefined): string {
  const data = event?.data as ChatMessageData | undefined;
  return String(data?.text || data?.insight || "")
    .replace(/\s+/g, " ")
    .trim();
}

function getSenderLabel(
  event: LedgerEvent | undefined,
  actorTitleById: Map<string, string>,
  you: string,
): string {
  if (!event) return "";
  const data = event.data as ChatMessageData | undefined;
  const by = String(event.by || "").trim();
  if (by === "user") return you;
  const resolved = String(data?.sender_title || "").trim() || actorTitleById.get(by);
  return resolved || by;
}

const ActorOverviewCard = memo(function ActorOverviewCard({
  groupId,
  actor,
  isDark,
  mode,
  actorStatusProvisional,
  stoppedSinceMs,
  mail,
  agentState,
  msgExcerpt,
  ttyLines,
  youLabel,
  actorTitleById,
  onOpenActor,
}: {
  groupId: string;
  actor: Actor;
  isDark: boolean;
  mode: ExcerptMode;
  stoppedSinceMs: number;
  actorStatusProvisional?: boolean;
  mail: RuntimeDockMailInfo | undefined;
  agentState: AgentState | undefined;
  msgExcerpt: LedgerEvent[];
  ttyLines: string[] | undefined;
  youLabel: string;
  actorTitleById: Map<string, string>;
  onOpenActor: (actorId: string) => void;
}) {
  const { t } = useTranslation(["chat", "actors"]);
  const { isRunning, workingState } = useActorDisplayState({
    groupId,
    actor,
    actorStatusProvisional,
  });
  const tone = toneFor(isRunning, workingState);
  const stateSince = useStateSinceMs(
    isRunning,
    workingState,
    actor.effective_working_updated_at,
    stoppedSinceMs,
  );
  const mailCount = mail?.count ?? 0;
  const now = useElapsedNow(stateSince !== null || mailCount > 0);
  const toneClasses = TONE_CLASSES[tone];
  const actorTitle = String(actor.title || actor.id || "").trim() || String(actor.id || "");
  const elapsedLabel =
    stateSince !== null ? formatElapsedCompact(Math.max(0, now - stateSince.ms)) : "";
  const statusLabel =
    tone === "stopped"
      ? t("chat:workView.overviewDown")
      : t(`actors:${toneClasses.labelKey}`, { defaultValue: tone });
  const pillText = elapsedLabel ? `${statusLabel} · ${elapsedLabel}` : statusLabel;
  const mailOldestLabel =
    mailCount > 0 && mail?.oldestTsMs
      ? `${Math.floor(Math.max(0, now - mail.oldestTsMs) / 60000)}m`
      : "";
  const taskId = String(agentState?.hot?.active_task_id || "").trim();
  const blockers = Array.isArray(agentState?.hot?.blockers) ? agentState.hot.blockers.length : 0;
  const focus = String(agentState?.hot?.focus || "").trim();
  const nextAction = String(agentState?.hot?.next_action || "").trim();

  return (
    <div
      role="button"
      tabIndex={0}
      onClick={() => onOpenActor(String(actor.id))}
      onKeyDown={(e) => {
        if (e.key === "Enter" || e.key === " ") {
          e.preventDefault();
          onOpenActor(String(actor.id));
        }
      }}
      className={classNames(
        "relative flex h-full min-h-0 cursor-pointer flex-col rounded-xl border px-3 py-2.5 transition-colors focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-[var(--color-text-secondary)]",
        actor.role === "foreman"
          ? "border-indigo-400/40 bg-indigo-500/[0.08]"
          : isDark
            ? "border-[var(--glass-panel-border)] bg-[var(--color-bg-primary)]/40 hover:bg-[var(--glass-tab-bg-hover)]"
            : "border-[var(--glass-panel-border)] bg-white/70 hover:bg-white",
      )}
      data-overview-actor-id={actor.id}
    >
      <div className="flex items-center gap-2.5">
        <span
          className={classNames(
            "flex shrink-0 items-center justify-center rounded-full p-[2px]",
            toneClasses.ring,
          )}
        >
          <ActorAvatar
            avatarUrl={actor.avatar_url || undefined}
            runtime={actor.runtime}
            title={actorTitle}
            isDark={isDark}
            sizeClassName="h-8 w-8"
            className={tone === "stopped" ? "opacity-45 grayscale saturate-50" : undefined}
          />
        </span>
        <span className="flex min-w-0 flex-col">
          <span className="truncate text-sm font-semibold text-[var(--color-text-primary)]">
            {actorTitle}
          </span>
          {actor.role ? (
            <span className="text-[10px] uppercase tracking-wide text-[var(--color-text-tertiary)]">
              {actor.role === "foreman" ? t("actors:foreman") : actor.role}
            </span>
          ) : null}
        </span>
        <span className="ml-auto flex shrink-0 items-center gap-1.5">
          {mailCount > 0 ? (
            <span
              className="inline-flex items-center gap-1 rounded-md bg-amber-400/90 px-1.5 py-0.5 text-[10px] font-bold text-amber-950"
              title={t("chat:runtimeDockUnreadMail", { count: mailCount, age: mailOldestLabel })}
            >
              {mailCount}
              <Mail className="h-2.5 w-2.5" strokeWidth={3} aria-hidden="true" />
              {mailOldestLabel}
            </span>
          ) : null}
          <span
            className={classNames(
              "inline-flex items-center rounded-md px-1.5 py-0.5 text-[10px] font-bold tabular-nums ring-1 ring-inset",
              TONE_PILL_CLASSES[tone],
            )}
          >
            {pillText}
          </span>
        </span>
      </div>
      {taskId || blockers > 0 ? (
        <div className="mt-1.5 flex flex-wrap items-center gap-1">
          {taskId ? (
            <span className="rounded-md border border-indigo-400/30 bg-indigo-500/10 px-1.5 py-px text-[10px] font-semibold text-indigo-300">
              {taskId}
            </span>
          ) : null}
          {blockers > 0 ? (
            <span className="rounded-md border border-rose-400/30 bg-rose-500/10 px-1.5 py-px text-[10px] font-semibold text-rose-300">
              {t("chat:workView.overviewBlockers", { count: blockers })}
            </span>
          ) : null}
        </div>
      ) : null}
      {focus ? (
        <div className="mt-1 truncate text-[11px] italic text-[var(--color-text-tertiary)]">
          {focus}
        </div>
      ) : null}
      <div
        className={classNames(
          "mt-2 min-h-0 flex-1 overflow-hidden rounded-lg border border-[var(--glass-border-subtle)] px-2.5 py-2 text-[12px] leading-[1.45]",
          isDark ? "bg-black/20 text-[var(--color-text-secondary)]" : "bg-slate-50 text-slate-600",
          mode === "tty" && "font-mono text-[11px]",
        )}
      >
        {mode === "tty" ? (
          ttyLines && ttyLines.length > 0 ? (
            <>
              {ttyLines.slice(0, -1).map((line, i) => (
                <div key={i} className="truncate text-[var(--color-text-tertiary)]">
                  {line}
                </div>
              ))}
              <div className="truncate">{ttyLines[ttyLines.length - 1]}</div>
            </>
          ) : (
            <span className="italic text-[var(--color-text-tertiary)]">
              {t("chat:workView.overviewNoTerminal")}
            </span>
          )
        ) : msgExcerpt.length > 0 ? (
          <>
            {msgExcerpt.map((ev, i) => (
              <div
                key={String(ev.id || i)}
                className={
                  i > 0
                    ? "mt-1.5 border-t border-dashed border-[var(--glass-border-subtle)] pt-1.5 text-[var(--color-text-tertiary)]"
                    : undefined
                }
              >
                <span className="float-right text-[10px] text-[var(--color-text-tertiary)]">
                  {ev.ts ? formatElapsedCompact(Math.max(0, now - Date.parse(ev.ts))) : ""}
                </span>
                <span className="font-semibold text-indigo-300/90">
                  {getSenderLabel(ev, actorTitleById, youLabel)}
                </span>
                : {getMessagePreview(ev)}
              </div>
            ))}
          </>
        ) : (
          <span className="italic text-[var(--color-text-tertiary)]">
            {t("chat:workView.overviewNoMessage")}
          </span>
        )}
        {mode === "msg" && nextAction ? (
          <div className="mt-1.5 border-t border-dashed border-[var(--glass-border-subtle)] pt-1.5 text-[11px] text-emerald-400/90">
            {t("chat:workView.overviewNext", { value: nextAction })}
          </div>
        ) : null}
      </div>
    </div>
  );
});

function buildDirectEdges(
  events: LedgerEvent[],
  actors: Actor[],
  actorTitleById: Map<string, string>,
  you: string,
): DirectEdge[] {
  const edges: DirectEdge[] = [];
  const seen = new Set<string>();
  for (let i = events.length - 1; i >= 0 && edges.length < MAX_EDGES; i -= 1) {
    const ev = events[i];
    if (!isChatMessageEvent(ev)) continue;
    const data = ev.data as ChatMessageData | undefined;
    if (typeof data?.dst_group_id === "string" && data.dst_group_id.trim()) continue;
    const recipients = getRecipientActorIdsForEvent(ev, actors);
    if (recipients.length !== 1) continue;
    const fromId = String(ev.by || "").trim();
    const toId = recipients[0];
    if (!fromId || fromId === toId) continue;
    const pairKey = `${fromId}→${toId}`;
    if (seen.has(pairKey)) continue;
    seen.add(pairKey);
    const preview = getMessagePreview(ev);
    edges.push({
      fromId,
      toId,
      label: `${getSenderLabel(ev, actorTitleById, you)} → ${actorTitleById.get(toId) || toId}: ${preview}`,
      isMail: data?.message_mode === "mail",
      rank: edges.length,
    });
  }
  return edges;
}

function lastInboundByActor(
  events: LedgerEvent[],
  actors: Actor[],
  depth: number,
): Map<string, LedgerEvent[]> {
  const map = new Map<string, LedgerEvent[]>();
  for (let i = events.length - 1; i >= 0; i -= 1) {
    const ev = events[i];
    if (!isChatMessageEvent(ev)) continue;
    for (const recipientId of getRecipientActorIdsForEvent(ev, actors)) {
      const list = map.get(recipientId);
      if (!list) {
        map.set(recipientId, [ev]);
      } else if (list.length < depth) {
        list.push(ev);
      }
    }
  }
  return map;
}

function tailLines(text: string, count: number): string[] {
  const lines = text
    .split(/\r?\n/)
    .map((line) => line.trimEnd())
    .filter((line) => line.trim().length > 0);
  return lines.slice(-count);
}

export function GroupOverview({
  groupId,
  actors,
  isDark,
  loading,
  actorStatusProvisional,
  onOpenActor,
}: Props) {
  const { t } = useTranslation(["chat", "actors"]);
  const events = useGroupStore((state) => selectChatBucketState(state, groupId).events);
  const [mode, setMode] = useState<ExcerptMode>("msg");
  const [page, setPage] = useState(0);
  const stageRef = useRef<HTMLDivElement | null>(null);
  const cellRefs = useRef<(HTMLDivElement | null)[]>([]);
  const [cellRects, setCellRects] = useState<CellRect[]>([]);

  const actorTitleById = useMemo(
    () =>
      new Map(
        actors.map((actor) => [String(actor.id || ""), String(actor.title || actor.id || "")]),
      ),
    [actors],
  );

  const foreman = useMemo(
    () => actors.find((a) => a.role === "foreman") ?? actors.find((a) => a.role !== "foreman"),
    [actors],
  );
  const peers = useMemo(() => actors.filter((a) => a !== foreman), [actors, foreman]);
  const pageCount = Math.max(1, Math.ceil(peers.length / PAGE_SIZE));
  const safePage = Math.min(page, pageCount - 1);
  const visiblePeers = peers.slice(safePage * PAGE_SIZE, safePage * PAGE_SIZE + PAGE_SIZE);

  const mailInfoByActorId = useMemo(() => buildRuntimeDockMailInfo(events), [events]);

  const liveStopped = useMemo(() => buildActorStoppedSinceMap(events), [events]);
  const [historyStopped, setHistoryStopped] = useState<Map<string, number | null> | null>(null);
  useEffect(() => {
    let cancelled = false;
    fetchLedgerTailAll(groupId, 200)
      .then((resp) => {
        if (cancelled || !resp?.ok || !resp.result) return;
        const tailEvents = Array.isArray((resp.result as { events?: unknown }).events)
          ? ((resp.result as { events: LedgerEvent[] }).events as LedgerEvent[])
          : [];
        if (!cancelled) setHistoryStopped(buildActorStoppedSinceMap(tailEvents));
      })
      .catch(() => undefined);
    return () => {
      cancelled = true;
    };
  }, [groupId]);
  const stoppedSinceByActorId = useMemo(() => {
    const merged = new Map<string, number | null>();
    if (historyStopped) {
      for (const [id, v] of historyStopped) merged.set(id, v);
    }
    for (const [id, v] of liveStopped) merged.set(id, v);
    return merged;
  }, [historyStopped, liveStopped]);

  const [agentStates, setAgentStates] = useState<Map<string, AgentState>>(new Map());
  useEffect(() => {
    let cancelled = false;
    const load = () => {
      fetchContext(groupId)
        .then((resp) => {
          if (cancelled || !resp?.ok || !resp.result) return;
          const next = new Map<string, AgentState>();
          for (const st of resp.result.agent_states || []) {
            if (st?.id) next.set(String(st.id), st);
          }
          if (!cancelled) setAgentStates(next);
        })
        .catch(() => undefined);
    };
    load();
    const id = window.setInterval(load, AGENT_STATE_POLL_MS);
    return () => {
      cancelled = true;
      window.clearInterval(id);
    };
  }, [groupId]);

  const inboundByActor = useMemo(() => lastInboundByActor(events, actors, 2), [events, actors]);

  const [ttyByActorId, setTtyByActorId] = useState<Map<string, string[]>>(new Map());
  useEffect(() => {
    if (mode !== "tty") return;
    let cancelled = false;
    const load = async () => {
      const ids = [foreman, ...visiblePeers]
        .filter((a): a is Actor => !!a)
        .map((a) => String(a.id || ""));
      const next = new Map<string, string[]>();
      await Promise.all(
        ids.map(async (actorId) => {
          try {
            const resp = await fetchTerminalTail(groupId, actorId, 1600, true, true);
            if (resp?.ok && resp.result?.text) {
              next.set(actorId, tailLines(resp.result.text, 3));
            }
          } catch {
            /* tail unavailable for this actor */
          }
        }),
      );
      if (!cancelled) setTtyByActorId(next);
    };
    void load();
    const id = window.setInterval(load, TERMINAL_POLL_MS);
    return () => {
      cancelled = true;
      window.clearInterval(id);
    };
  }, [mode, groupId, foreman, visiblePeers]);

  const youLabel = t("chat:workView.overviewFromYou");
  const edges = useMemo(
    () => buildDirectEdges(events, actors, actorTitleById, youLabel),
    [events, actors, actorTitleById, youLabel],
  );

  /* visible cell assignment: foreman at 4, peers through GRID_SLOTS */
  const cellActor = useMemo(() => {
    const map = new Map<number, Actor>();
    if (foreman) map.set(4, foreman);
    visiblePeers.forEach((a, i) => {
      if (i < GRID_SLOTS.length) map.set(GRID_SLOTS[i], a);
    });
    return map;
  }, [foreman, visiblePeers]);

  useLayoutEffect(() => {
    const stage = stageRef.current;
    if (!stage) return;
    const measure = () => {
      const sr = stage.getBoundingClientRect();
      setCellRects(
        cellRefs.current.map((el) => {
          if (!el) return null as unknown as CellRect;
          const r = el.getBoundingClientRect();
          return {
            l: r.left - sr.left,
            r: r.right - sr.left,
            t: r.top - sr.top,
            b: r.bottom - sr.top,
            cx: (r.left + r.right) / 2 - sr.left,
            cy: (r.top + r.bottom) / 2 - sr.top,
          };
        }),
      );
    };
    measure();
    const ro = new ResizeObserver(measure);
    ro.observe(stage);
    return () => ro.disconnect();
  }, [mode, safePage, actors.length]);

  const edgesRendered = useMemo(() => {
    if (cellRects.length < 9) return [];
    const cellOf = new Map<Actor, number>();
    for (const [cell, actor] of cellActor) cellOf.set(actor, cell);
    return edges
      .map((edge) => {
        const fromActor = actors.find((a) => String(a.id) === edge.fromId);
        const toActor = actors.find((a) => String(a.id) === edge.toId);
        const fromCell = edge.fromId === "user" ? 4 : fromActor ? cellOf.get(fromActor) : undefined;
        const toCell = toActor ? cellOf.get(toActor) : undefined;
        if (fromCell === undefined || toCell === undefined) return null;
        const s = cellRects[fromCell];
        const d = cellRects[toCell];
        if (!s || !d) return null;
        const sameRow = Math.abs(s.cy - d.cy) < 40;
        const sameCol = Math.abs(s.cx - d.cx) < 40;
        let path: string;
        let lx: number;
        let ly: number;
        if (sameRow) {
          const ltr = d.cx > s.cx;
          const sx = ltr ? s.r : s.l;
          const ex = ltr ? d.l : d.r;
          const y = (s.cy + d.cy) / 2;
          path = `M ${sx} ${y} H ${ex}`;
          lx = (sx + ex) / 2;
          ly = y;
        } else if (sameCol) {
          const up = d.cy < s.cy;
          const sy = up ? s.t : s.b;
          const ey = up ? d.b : d.t;
          const x = (s.cx + d.cx) / 2;
          path = `M ${x} ${sy} V ${ey}`;
          lx = x;
          ly = (sy + ey) / 2;
        } else {
          const ltr = d.cx > s.cx;
          const sx = ltr ? s.r : s.l;
          const ex = ltr ? d.l : d.r;
          const mx = (sx + ex) / 2;
          path = `M ${sx} ${s.cy} H ${mx} V ${d.cy} H ${ex}`;
          lx = mx;
          ly = (s.cy + d.cy) / 2;
        }
        return { edge, path, lx, ly };
      })
      .filter((e): e is NonNullable<typeof e> => !!e);
  }, [cellRects, cellActor, edges, actors]);

  const corners = useMemo(() => {
    if (cellRects.length < 9) return [];
    /* interior junctions: (r,c) in {0,1}x{0,1} — cells (r,c),(r,c+1),(r+1,c),(r+1,c+1) */
    const out: { x: number; y: number; text: string }[] = [];
    for (const [r, c] of [
      [0, 0],
      [0, 1],
      [1, 0],
      [1, 1],
    ] as const) {
      const adjacent = [r * 3 + c, r * 3 + c + 1, (r + 1) * 3 + c, (r + 1) * 3 + c + 1]
        .map((cell) => cellActor.get(cell))
        .filter((a): a is Actor => !!a);
      if (!adjacent.length) continue;
      let text = "";
      for (const a of adjacent) {
        const st = agentStates.get(String(a.id));
        const blk = Array.isArray(st?.hot?.blockers) ? st.hot.blockers.length : 0;
        const mail = mailInfoByActorId.get(String(a.id))?.count ?? 0;
        const title = String(a.title || a.id || "");
        if (blk > 0) {
          text = `${title} · ⚠${blk}${mail > 0 ? ` + ${mail}✉` : ""}`;
          break;
        }
        if (mail > 0 && !text) text = `${title} · ${mail}✉`;
      }
      if (!text) {
        const working = adjacent.filter((a) => {
          const ws = String(a.effective_working_state || "").toLowerCase();
          return ws === "working" || ws === "waiting" || ws === "stuck";
        }).length;
        if (working > 0) {
          text = t("chat:workView.overviewWorkingCount", { count: working });
        }
      }
      if (!text) continue;
      const x = (cellRects[r * 3 + c].r + cellRects[r * 3 + c + 1].l) / 2;
      const y = (cellRects[r * 3 + c].b + cellRects[(r + 1) * 3 + c].t) / 2;
      out.push({ x, y, text });
    }
    return out;
  }, [cellRects, cellActor, agentStates, mailInfoByActorId, t]);

  return (
    <div className="flex min-h-0 flex-1 flex-col" data-group-overview>
      <div className="flex shrink-0 items-center justify-between gap-3 border-b border-[var(--glass-border-subtle)] px-4 py-2">
        <span className="text-xs text-[var(--color-text-tertiary)]">
          {t("chat:workView.overviewActors", { count: actors.length })}
        </span>
        <div className="flex items-center gap-2">
          {pageCount > 1 ? (
            <span className="text-[11px] tabular-nums text-[var(--color-text-tertiary)]">
              <button
                type="button"
                className="px-1 disabled:opacity-40"
                disabled={safePage <= 0}
                onClick={() => setPage(safePage - 1)}
                aria-label={t("chat:workView.overviewPrevPage")}
              >
                ‹
              </button>
              {t("chat:workView.overviewPage", {
                from: safePage * PAGE_SIZE + 1,
                to: Math.min((safePage + 1) * PAGE_SIZE, peers.length),
                total: peers.length,
              })}
              <button
                type="button"
                className="px-1 disabled:opacity-40"
                disabled={safePage >= pageCount - 1}
                onClick={() => setPage(safePage + 1)}
                aria-label={t("chat:workView.overviewNextPage")}
              >
                ›
              </button>
            </span>
          ) : null}
          <div
            className="flex overflow-hidden rounded-md ring-1 ring-inset ring-[var(--glass-border-subtle)]"
            role="group"
            aria-label={t("chat:workView.overviewSource")}
          >
            {(["msg", "tty"] as const).map((value) => (
              <button
                key={value}
                type="button"
                aria-pressed={mode === value}
                onClick={() => setMode(value)}
                className={classNames(
                  "h-7 px-2.5 text-xs transition-colors focus-visible:outline-2 focus-visible:outline-[var(--color-text-secondary)]",
                  mode === value
                    ? "bg-[var(--glass-tab-bg-active)] font-semibold text-[var(--color-text-primary)]"
                    : "text-[var(--color-text-tertiary)] hover:bg-[var(--glass-tab-bg)]",
                )}
                data-overview-mode={value}
              >
                {value === "msg"
                  ? t("chat:workView.overviewSegMessages")
                  : t("chat:workView.overviewSegTerminal")}
              </button>
            ))}
          </div>
        </div>
      </div>
      <div className="min-h-0 flex-1 px-3 py-3 sm:px-4">
        {actors.length === 0 ? (
          <div className="flex min-h-0 items-center justify-center p-6 text-center text-sm text-[var(--color-text-tertiary)]">
            {t(loading ? "chat:workView.loading" : "chat:workView.empty")}
          </div>
        ) : (
          <div ref={stageRef} className="relative h-full min-h-[420px]">
            <div className="absolute inset-0 grid grid-cols-3 grid-rows-3 gap-x-9 gap-y-8">
              {Array.from({ length: 9 }, (_, cell) => {
                const actor = cellActor.get(cell);
                return (
                  <div
                    key={cell}
                    ref={(el) => {
                      cellRefs.current[cell] = el;
                    }}
                    className={classNames("min-h-0 min-w-0", !actor && "invisible")}
                  >
                    {actor ? (
                      <ActorOverviewCard
                        groupId={groupId}
                        actor={actor}
                        isDark={isDark}
                        mode={mode}
                        actorStatusProvisional={actorStatusProvisional}
                        stoppedSinceMs={stoppedSinceByActorId.get(String(actor.id)) ?? 0}
                        mail={mailInfoByActorId.get(String(actor.id))}
                        agentState={agentStates.get(String(actor.id))}
                        msgExcerpt={inboundByActor.get(String(actor.id)) || []}
                        ttyLines={ttyByActorId.get(String(actor.id))}
                        youLabel={youLabel}
                        actorTitleById={actorTitleById}
                        onOpenActor={onOpenActor}
                      />
                    ) : null}
                  </div>
                );
              })}
            </div>
            <svg className="pointer-events-none absolute inset-0 h-full w-full" aria-hidden="true">
              {edgesRendered.map(({ edge, path }) => (
                <path
                  key={`${edge.fromId}-${edge.toId}`}
                  d={path}
                  fill="none"
                  stroke={edge.isMail ? "rgba(251, 191, 36, 0.6)" : "rgba(148, 163, 184, 0.5)"}
                  strokeWidth={edge.rank < 3 ? 2 : 1.5}
                  strokeDasharray={edge.isMail ? "5 4" : undefined}
                  strokeLinejoin="round"
                  opacity={Math.max(0.3, 1 - edge.rank * 0.09)}
                />
              ))}
            </svg>
            {edgesRendered.map(({ edge, lx, ly }) => (
              <div
                key={`label-${edge.fromId}-${edge.toId}`}
                className="pointer-events-none absolute max-w-[170px] -translate-x-1/2 -translate-y-1/2 truncate rounded-md border border-[var(--glass-border-subtle)] bg-[var(--color-bg-primary)] px-1.5 py-0.5 text-[9px] text-[var(--color-text-tertiary)]"
                style={{ left: lx, top: ly } as CSSProperties}
                title={edge.label}
              >
                {edge.label}
              </div>
            ))}
            {corners.map((corner, i) => (
              <div
                key={i}
                className="pointer-events-none absolute max-w-[180px] -translate-x-1/2 -translate-y-1/2 text-center text-[9.5px] italic leading-[1.35] text-[var(--color-text-tertiary)]"
                style={{ left: corner.x, top: corner.y } as CSSProperties}
              >
                {corner.text}
              </div>
            ))}
          </div>
        )}
      </div>
    </div>
  );
}
