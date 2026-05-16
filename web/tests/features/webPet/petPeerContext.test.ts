import { describe, expect, it } from "vitest";
import { buildPetPeerContext, shouldFetchPetPeerContext } from "../../../src/features/webPet/petPeerContext";

describe("petPeerContext", () => {
  it("does not fetch pet context when the web pet is disabled", () => {
    expect(shouldFetchPetPeerContext({ groupId: "g-demo", enabled: false })).toBe(false);
    expect(shouldFetchPetPeerContext({ groupId: "g-demo", enabled: true })).toBe(true);
    expect(shouldFetchPetPeerContext({ groupId: "", enabled: true })).toBe(false);
  });

  it("maps pet decisions into reminders", () => {
    const context = buildPetPeerContext({
      decisions: [
        {
          id: "dec-1",
          kind: "actor_down",
          priority: 95,
          summary: "peer-1 最近出现执行错误，建议重启。",
          agent: "peer-1",
          fingerprint: "group:g-1:actor_down:peer-1",
          source: {
            actor_id: "peer-1",
            actor_role: "peer",
            task_id: "T1",
            error_reason: "Could not process image",
          },
          action: {
            type: "restart_actor",
            group_id: "g-1",
            actor_id: "peer-1",
          },
        },
      ],
      persona: "Keep low-noise. Auto restart actors.",
      help: "## Pet Persona\nKeep low-noise. Auto restart actors.",
      prompt: "You are the group's independent pet peer.\nRuntime Snapshot:\nGroup: Demo Team\nAgent Snapshot: foreman: T1 | close loop",
      snapshot: "Group: Demo Team\nAgent Snapshot: foreman: T1 | close loop",
      source: "help",
    });

    expect(context.source).toBe("help");
    expect(context.help).toContain("## Pet Persona");
    expect(context.prompt).toContain("independent pet peer");
    expect(context.snapshot).toContain("Group: Demo Team");
    expect(context.decisions[0]?.source.actorId).toBe("peer-1");
    expect(context.decisions[0]?.action.type).toBe("restart_actor");
  });

  it("falls back to default source when persona is empty", () => {
    const context = buildPetPeerContext(null);

    expect(context.source).toBe("default");
    expect(context.decisions).toEqual([]);
    expect(context.prompt).toBe("");
    expect(context.status).toBe("loaded");
  });

  it("maps task proposal decisions for foreman handoff", () => {
    const context = buildPetPeerContext({
      decisions: [
        {
          id: "dec-task-1",
          kind: "suggestion",
          priority: 88,
          summary: "建议让 foreman 推进 T315。",
          confidence: "medium",
          reasoning_brief: "T315 仍未推进，但当前焦点已经回到这条任务。",
          agent: "pet-peer",
          fingerprint: "group:g-1:suggestion:task-proposal:T315",
          source: {
            task_id: "T315",
          },
          action: {
            type: "task_proposal",
            group_id: "g-1",
            operation: "move",
            task_id: "T315",
            status: "active",
          },
        },
      ],
      persona: "Keep low-noise.",
      help: "",
      prompt: "",
      snapshot: "",
      source: "default",
    });

    expect(context.decisions[0]?.action.type).toBe("task_proposal");
    if (context.decisions[0]?.action.type === "task_proposal") {
      expect(context.decisions[0].action.operation).toBe("move");
      expect(context.decisions[0].action.taskId).toBe("T315");
      expect(context.decisions[0].action.status).toBe("active");
    }
    expect(context.decisions[0]?.confidence).toBe("medium");
    expect(context.decisions[0]?.reasoningBrief).toContain("T315");
  });

  it("maps draft_message decisions from action.text", () => {
    const context = buildPetPeerContext({
      decisions: [
        {
          id: "dec-suggestion-1",
          kind: "suggestion",
          priority: 72,
          summary: "建议直接发送这条回复。",
          agent: "pet-peer",
          fingerprint: "group:g-1:suggestion:reply-1",
          source: {
            event_id: "evt-1",
            suggestion_kind: "reply_required",
          },
          action: {
            type: "draft_message",
            group_id: "g-1",
            text: "请直接跟进用户。",
            reply_to: "evt-1",
            to: ["user"],
          },
        },
      ],
      persona: "Keep low-noise.",
      help: "",
      prompt: "",
      snapshot: "",
      source: "default",
    });

    expect(context.decisions).toHaveLength(1);
    expect(context.decisions[0]?.action.type).toBe("draft_message");
    if (context.decisions[0]?.action.type === "draft_message") {
      expect(context.decisions[0].action.text).toBe("请直接跟进用户。");
      expect(context.decisions[0].action.replyTo).toBe("evt-1");
    }
  });

  it("ignores unsupported automation proposal decisions in the V1 web surface", () => {
    const context = buildPetPeerContext({
      decisions: [
        {
          id: "dec-auto-1",
          kind: "suggestion",
          priority: 77,
          summary: "建议创建一个临时 automation rule。",
          agent: "pet-peer",
          fingerprint: "group:g-1:suggestion:auto-proposal",
          action: {
            type: "automation_proposal",
            group_id: "g-1",
            title: "Temporary nudge",
            summary: "Create a short-lived notify rule.",
            actions: [
              {
                type: "create_rule",
                rule: {
                  id: "pet-temp-nudge",
                  enabled: true,
                },
              },
            ],
          },
        },
      ],
      persona: "Keep low-noise.",
      help: "",
      prompt: "",
      snapshot: "",
      source: "default",
    });

    expect(context.decisions).toEqual([]);
  });

  it("preserves explicit loading and error status", () => {
    expect(buildPetPeerContext(null, { status: "loading" }).status).toBe("loading");
    expect(buildPetPeerContext(null, { status: "error" }).status).toBe("error");
  });

  it("maps signal payload", () => {
    const context = buildPetPeerContext({
      decisions: [],
      task_evidence: [
        {
          kind: "ownership_drift",
          priority: 78,
          hypothesis: "The assigned task looks stale while the actor is mounted on another task.",
          actor: {
            id: "backend-peer",
            active_task_id: "T107",
            focus: "Fix 400 bucket issue",
            blockers: ["waiting sample payload"],
          },
          task: {
            id: "T035",
            title: "图谱/API 后端 400 分桶与修复",
            status: "active",
            assignee: "backend-peer",
            updated_at: "2026-04-01T10:00:00.000Z",
          },
          current_active_task: {
            id: "T107",
            title: "Fix 400 bucket issue",
            status: "active",
          },
          signals: {
            task_stale_minutes: 42,
            same_workstream_hint: false,
            blocker_count: 1,
          },
        },
      ],
      persona: "",
      help: "",
      prompt: "",
      snapshot: "",
      source: "default",
      signals: {
        reply_pressure: {
          severity: "high",
          pending_count: 3,
          overdue_count: 1,
          oldest_pending_seconds: 1200,
          baseline_median_reply_seconds: 240,
        },
        coordination_rhythm: {
          severity: "medium",
          foreman_id: "foreman-1",
          silence_seconds: 600,
          baseline_median_gap_seconds: 180,
        },
        task_pressure: {
          severity: "high",
          score: 11,
          trend_score: 6,
          blocked_count: 2,
          waiting_user_count: 1,
          handoff_count: 1,
          planned_backlog_count: 0,
          recent_blocked_updates: 1,
          recent_waiting_user_updates: 1,
          recent_handoff_updates: 1,
          recent_task_create_ops: 1,
          recent_task_update_ops: 2,
          recent_task_move_ops: 1,
          recent_task_restore_ops: 0,
          recent_task_delete_ops: 1,
          recent_task_change_count: 5,
          recent_task_context_sync_events: 2,
          ledger_trend_score: 5,
        },
        proposal_ready: {
          ready: true,
          focus: "waiting_user",
          severity: "high",
          summary: "User-facing task is waiting; prefer one task proposal that helps foreman close the user dependency.",
          pending_reply_count: 1,
          overdue_reply_count: 0,
          waiting_user_count: 1,
          blocked_count: 2,
          handoff_count: 1,
          recent_task_change_count: 5,
          foreman_silence_seconds: 600,
        },
      },
    });

    expect(context.signals.replyPressure.pendingCount).toBe(3);
    expect(context.signals.replyPressure.severity).toBe("high");
    expect(context.signals.coordinationRhythm.foremanId).toBe("foreman-1");
    expect(context.signals.taskPressure.score).toBe(11);
    expect(context.signals.taskPressure.trendScore).toBe(6);
    expect(context.signals.taskPressure.recentTaskCreateOps).toBe(1);
    expect(context.signals.taskPressure.recentTaskChangeCount).toBe(5);
    expect(context.signals.taskPressure.ledgerTrendScore).toBe(5);
    expect(context.taskEvidence).toHaveLength(1);
    expect(context.taskEvidence[0]?.kind).toBe("ownership_drift");
    expect(context.taskEvidence[0]?.actor.activeTaskId).toBe("T107");
    expect(context.taskEvidence[0]?.task.id).toBe("T035");
    expect(context.taskEvidence[0]?.signals.taskStaleMinutes).toBe(42);
    expect(context.signals.proposalReady.ready).toBe(true);
    expect(context.signals.proposalReady.focus).toBe("waiting_user");
    expect(context.signals.proposalReady.severity).toBe("high");
  });
});
