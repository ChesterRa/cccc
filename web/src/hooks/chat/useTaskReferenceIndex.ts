import { useEffect, useMemo, useRef, useState } from "react";
import { fetchTasksByIds } from "../../services/api";
import type { LedgerEvent, Task } from "../../types";
import { getTaskMessageRefs } from "../../utils/taskRefs";

const TASK_ID_BATCH_SIZE = 100;
const TASK_FETCH_RETRY_DELAY_MS = 1_000;

export function collectTaskReferenceIds(events: readonly LedgerEvent[]): string[] {
  const ids = new Set<string>();
  for (const event of events) {
    const refs = getTaskMessageRefs((event.data as { refs?: unknown } | undefined)?.refs);
    for (const ref of refs) {
      const id = String(ref.task_id || "").trim();
      if (id) ids.add(id);
    }
  }
  return Array.from(ids).sort();
}

function taskMap(tasks: readonly Task[]): Map<string, Task> {
  const result = new Map<string, Task>();
  for (const task of tasks) {
    const id = String(task.id || "").trim();
    if (id) result.set(id, task);
  }
  return result;
}

export function useTaskReferenceIndex({
  groupId,
  events,
  tasksVersion,
  seedTasks,
}: {
  groupId: string;
  events: readonly LedgerEvent[];
  tasksVersion?: string;
  seedTasks?: readonly Task[];
}): Map<string, Task> {
  const ids = useMemo(() => collectTaskReferenceIds(events), [events]);
  const idsKey = ids.join("\u0000");
  const [byId, setById] = useState(() => taskMap(seedTasks || []));
  const [retryKey, setRetryKey] = useState(0);
  const byIdRef = useRef(byId);
  const fetchedVersionRef = useRef("");
  byIdRef.current = byId;

  useEffect(() => {
    const seeded = taskMap(seedTasks || []);
    byIdRef.current = seeded;
    fetchedVersionRef.current = "";
    setById(seeded);
  }, [groupId, seedTasks]);

  useEffect(() => {
    const stableIds = idsKey ? idsKey.split("\u0000") : [];
    if (!groupId || stableIds.length === 0) return;
    const version = String(tasksVersion || "").trim();
    const refreshAll = Boolean(version && version !== fetchedVersionRef.current);
    const wanted = refreshAll ? stableIds : stableIds.filter((id) => !byIdRef.current.has(id));
    if (wanted.length === 0) {
      if (version) fetchedVersionRef.current = version;
      return;
    }
    const controller = new AbortController();
    let retryTimer: number | undefined;
    const scheduleRetry = () => {
      if (controller.signal.aborted || retryTimer !== undefined) return;
      retryTimer = window.setTimeout(
        () => setRetryKey((value) => value + 1),
        TASK_FETCH_RETRY_DELAY_MS,
      );
    };
    const batches = Array.from(
      { length: Math.ceil(wanted.length / TASK_ID_BATCH_SIZE) },
      (_, index) => wanted.slice(index * TASK_ID_BATCH_SIZE, (index + 1) * TASK_ID_BATCH_SIZE),
    );
    void Promise.all(
      batches.map((batch) => fetchTasksByIds(groupId, batch, controller.signal)),
    ).then((responses) => {
      if (controller.signal.aborted) return;
      const complete = responses.every((response) => response.ok);
      const next = new Map(byIdRef.current);
      if (refreshAll && complete) next.clear();
      for (const response of responses) {
        if (!response.ok) continue;
        for (const task of response.result.tasks) next.set(task.id, task);
      }
      byIdRef.current = next;
      setById(next);
      if (complete && version) {
        fetchedVersionRef.current = version;
      }
      if (!complete) scheduleRetry();
    }, scheduleRetry);
    return () => {
      controller.abort();
      if (retryTimer !== undefined) window.clearTimeout(retryTimer);
    };
  }, [groupId, idsKey, retryKey, tasksVersion]);

  return byId;
}
