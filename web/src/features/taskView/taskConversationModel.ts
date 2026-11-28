import type { LedgerEvent, MessageRef, Task, TaskMessageRef } from "../../types";
import { getTaskMessageRefs } from "../../utils/taskRefs";

type MessageData = { text?: unknown; refs?: unknown; reply_to?: unknown };

function dataOf(event: LedgerEvent): MessageData {
  return event.data && typeof event.data === "object" ? (event.data as MessageData) : {};
}

function escapeForPattern(value: string): string {
  return value.replace(/[.*+?^${}()|[\]\\]/g, "\\$&");
}

/** The message carries a structured reference to the task. */
export function hasTaskRef(event: LedgerEvent, taskId: string): boolean {
  return getTaskMessageRefs(dataOf(event).refs).some(
    (ref) => String(ref.task_id || "").trim() === taskId,
  );
}

/**
 * Tells whether a message's own text names the task as a whole word: "T98" is not found in
 * "T981", nor inside a longer identifier or hash. Letter case is ignored, as people type
 * "t98". The pattern is built once and reused for every message of a pass.
 */
function taskMentionTest(taskId: string): (event: LedgerEvent) => boolean {
  if (!taskId) return () => false;
  // No lookbehind: older Safari cannot parse one.
  const pattern = new RegExp(`(^|[^A-Za-z0-9_])${escapeForPattern(taskId)}(?![A-Za-z0-9_])`, "i");
  return (event) => {
    const text = dataOf(event).text;
    return typeof text === "string" && !!text && pattern.test(text);
  };
}

export function mentionsTask(event: LedgerEvent, taskId: string): boolean {
  return taskMentionTest(taskId)(event);
}

/** One test for "this message belongs to the task", to run over many messages. */
export function taskMembershipTest(taskId: string): (event: LedgerEvent) => boolean {
  const mentions = taskMentionTest(taskId);
  return (event) => hasTaskRef(event, taskId) || mentions(event);
}

export function belongsToTask(event: LedgerEvent, taskId: string): boolean {
  return taskMembershipTest(taskId)(event);
}

function replyTarget(event: LedgerEvent): string {
  const target = dataOf(event).reply_to;
  return typeof target === "string" ? target.trim() : "";
}

export type TaskConversation = {
  events: LedgerEvent[];
  /**
   * A shown message replies to one that is neither a search result nor loaded, so some
   * context around the task is missing from the view.
   */
  contextIncomplete: boolean;
};

/**
 * The messages of one task: those that reference or name it, from the server search over the
 * whole history and from the loaded conversation, plus what each answers (its ancestors) and
 * the replies under it (its descendants). A chain is followed only through messages already at
 * hand (search results and the loaded conversation); a parent outside them is reported, not
 * fetched.
 */
export function collectTaskConversation(
  searched: readonly LedgerEvent[],
  loaded: readonly LedgerEvent[],
  taskId: string,
): TaskConversation {
  const belongs = taskMembershipTest(taskId);
  const byId = new Map<string, LedgerEvent>();
  const anonymous: LedgerEvent[] = [];
  // The loaded copy wins: it carries live read and reply status.
  for (const event of [...searched, ...loaded]) {
    const id = String(event.id || "").trim();
    if (id) byId.set(id, event);
    else if (belongs(event)) anonymous.push(event);
  }
  // The messages that are the task's own; everything else shown is context around them.
  const seeds: string[] = [];
  for (const [id, event] of byId) if (belongs(event)) seeds.push(id);
  const shown = new Set<string>(seeds);

  const repliesTo = new Map<string, string[]>();
  for (const [id, event] of byId) {
    const target = replyTarget(event);
    if (target) repliesTo.set(target, [...(repliesTo.get(target) ?? []), id]);
  }
  let contextIncomplete = false;
  // Upward: what each task message answers, and what that answered in turn.
  for (const seed of seeds) {
    for (let parent = replyTarget(byId.get(seed)!); parent; ) {
      const event = byId.get(parent);
      if (!event) {
        contextIncomplete = true;
        break;
      }
      if (shown.has(parent)) break;
      shown.add(parent);
      parent = replyTarget(event);
    }
  }
  // Downward: the replies to a task message, and the replies to those. An ancestor's other
  // replies stay out: one question answered by several members is not one thread.
  const queue = [...seeds];
  while (queue.length) {
    for (const reply of repliesTo.get(queue.pop()!) ?? []) {
      if (shown.has(reply)) continue;
      shown.add(reply);
      queue.push(reply);
    }
  }
  const events = [...[...shown].map((id) => byId.get(id)!), ...anonymous].sort((a, b) =>
    String(a.ts || "").localeCompare(String(b.ts || "")),
  );
  return { events, contextIncomplete };
}

/** The reference a message sent from the task's view carries, as tracked sends write it. */
export function taskViewRef(taskId: string, task?: Task | null, title?: string): TaskMessageRef {
  return {
    kind: "task_ref",
    task_id: taskId,
    title: String(task?.title || title || "").trim() || undefined,
    status: String(task?.status || "").trim() || undefined,
    waiting_on: task?.waiting_on ?? undefined,
    handoff_to: task?.handoff_to ?? undefined,
  } as TaskMessageRef;
}

/** A message's refs with the task's reference added, unless one is already among them. */
export function withTaskViewRef(
  refs: readonly MessageRef[],
  taskId: string,
  title?: string,
): MessageRef[] {
  const present = getTaskMessageRefs(refs).some(
    (item) => String(item.task_id || "").trim() === taskId,
  );
  return present ? [...refs] : [...refs, taskViewRef(taskId, null, title)];
}
