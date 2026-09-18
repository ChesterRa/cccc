// Real ContextModal; deterministic local transport, no daemon or provider.
import { useState } from "react";
import { ContextModal } from "../../src/components/ContextModal";
import type { GroupContext, Task } from "../../src/types";
import i18n from "../../src/i18n";
import "../../src/index.css";

const params = new URLSearchParams(location.search);
const dark = params.get("theme") === "dark";
document.documentElement.classList.add(dark ? "dark" : "light");
document.documentElement.style.fontSize = `${params.get("scale") || 100}%`;
await i18n.changeLanguage(params.get("lang") || "en");
const sparse = params.get("data") !== "full";
let context: GroupContext = {
  tasks_version: "tasksv:1",
  coordination: {
    brief: sparse
      ? {}
      : {
          objective: "Prepare the next release with verified collaboration workflows",
          current_focus: "Review handoffs and resolve the remaining browser regression",
          project_brief:
            "The terminal and file-reading workflows are verified. Review the outstanding items before release.",
          constraints: ["Preserve drafts", "No production service interruption"],
          updated_at: new Date().toISOString(),
        },
  },
  agent_states: Array.from({ length: 5 }, (_, i) => ({
    id: i === 0 ? "管理员" : `agent-${i}`,
    updated_at: new Date(Date.now() - (i === 0 ? 120_000 : 86_400_000)).toISOString(),
    hot: {
      focus: "Verify the collaboration changes and preserve existing work.",
      next_action: "Review evidence before accepting the next task.",
      active_task_id: i === 1 ? "T002" : "",
      blockers: i === 0 ? ["Waiting for the review result"] : [],
    },
    warm: {
      environment_summary:
        "The project has work in progress. Confirm the current branch and ownership before making edits. ".repeat(
          4,
        ),
      user_model: "Prefer focused changes backed by reproducible evidence.",
      persona_notes: "Stay idle until a concrete task is assigned.",
      open_loops: ["Check the pending review"],
      commitments: ["Preserve user drafts"],
    },
  })),
};
let tasks: Task[] = Array.from({ length: sparse ? 2 : 10 }, (_, i) => ({
  id: `T${String(i + 1).padStart(3, "0")}`,
  title:
    i === 0
      ? "Verify terminal navigation and retained session dimensions"
      : `Review workflow ${i + 1}`,
  status: sparse ? "done" : ["planned", "active", "done"][i % 3],
  assignee: "管理员",
  task_type: "standard",
  outcome:
    "The focused checks passed; retain the existing user session and review the browser evidence.",
  notes: "",
}));
const errors: string[] = [];
window.addEventListener("error", (event) => errors.push(event.message));
window.addEventListener("unhandledrejection", (event) => errors.push(String(event.reason)));
const requests: { path: string; method: string; body: unknown }[] = [];
window.fetch = async (input, init) => {
  const url = new URL(
    typeof input === "string" ? input : input instanceof URL ? input.href : input.url,
    location.origin,
  );
  const method = init?.method || "GET";
  const body = init?.body ? JSON.parse(String(init.body)) : null;
  requests.push({ path: url.pathname, method, body });
  let result: unknown;
  if (url.pathname.endsWith("/tasks")) {
    const query = (url.searchParams.get("query") || "").toLowerCase();
    const attention = url.searchParams.get("attention");
    const matches = tasks.filter(
      (task) => !attention && `${task.title} ${task.id}`.toLowerCase().includes(query),
    );
    result = {
      tasks_version: context.tasks_version,
      task_index: tasks,
      facets: {
        status_counts: Object.fromEntries(
          ["planned", "active", "done", "archived"].map((status) => [
            status,
            tasks.filter((t) => t.status === status).length,
          ]),
        ),
        assignees: ["管理员"],
      },
      pages: Object.fromEntries(
        ["planned", "active", "done", "archived"].map((status) => {
          const items = matches.filter((t) => t.status === status);
          return [
            status,
            {
              tasks: items,
              count: items.length,
              total_count: items.length,
              offset: 0,
              has_more: false,
            },
          ];
        }),
      ),
    };
  } else if (/\/tasks\/[^/]+$/.test(url.pathname)) {
    result = {
      task: tasks.find((t) => t.id === url.pathname.split("/").at(-1)),
      tasks_version: context.tasks_version,
      delete_info: { allowed: true, total: 1 },
    };
  } else if (url.pathname.endsWith("/context")) {
    for (const op of body?.ops || []) {
      if (op.op === "coordination.brief.update")
        context = {
          ...context,
          coordination: {
            ...context.coordination,
            brief: { ...op, updated_at: new Date().toISOString() },
          },
        };
      if (op.op === "task.update")
        tasks = tasks.map((t) => (t.id === op.task_id ? { ...t, ...op } : t));
    }
    result = context;
  } else if (url.pathname.endsWith("/project_md")) {
    result = {
      found: true,
      content: "# Project\n\nRepository reference. Keep current decisions in the working summary.",
      path: "/fixture/PROJECT.md",
    };
  } else {
    throw new Error(`Unmocked fixture request: ${method} ${url.pathname}`);
  }
  return Response.json({ ok: true, result });
};

export function Fixture() {
  const [value, setValue] = useState(context);
  const [open, setOpen] = useState(true);
  Object.assign(window, {
    contextWorkProbe: {
      requests,
      errors,
      t: (key: string) => i18n.t(`modals:context.${key}`),
      language: (lang: string) => i18n.changeLanguage(lang),
    },
  });
  return (
    <>
      <button onClick={() => setOpen(true)}>Open context</button>
      <ContextModal
        isOpen={open}
        onClose={() => setOpen(false)}
        groupId="g_fixture"
        context={value}
        onOpenContext={async () => setValue(context)}
        onSyncContext={async () => setValue(context)}
        isDark={dark}
      />
    </>
  );
}
