import { useCallback, useEffect, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import type { SecretaryTaskSummary } from "../../../types";
import * as api from "../../../services/api";
import { copyTextToClipboard } from "../../../utils/copy";

export type SecretaryTaskAction = "cancel" | "retry" | "candidate" | "handoff";

export type SecretaryTaskCoverage = {
  deferred: number;
  held: number;
  invalid: number;
  unprocessed: number;
  configured: boolean;
  ready: boolean;
  readiness_code: api.SecretaryReadinessCode | null;
  readiness_error: string;
};

const EMPTY_COVERAGE: SecretaryTaskCoverage = {
  deferred: 0,
  held: 0,
  invalid: 0,
  unprocessed: 0,
  configured: true,
  ready: true,
  readiness_code: null,
  readiness_error: "",
};

export function useSecretaryTasks(
  groupId: string,
  active: boolean,
  onRetryAccepted?: (task: SecretaryTaskSummary) => void,
) {
  const { t } = useTranslation("settings");
  const [tasks, setTasks] = useState<SecretaryTaskSummary[]>([]);
  const [coverage, setCoverage] = useState<SecretaryTaskCoverage>(EMPTY_COVERAGE);
  const [error, setError] = useState("");
  const [loadError, setLoadError] = useState("");
  const [busy, setBusy] = useState("");
  const [followups, setFollowups] = useState<Record<string, string>>({});
  const [candidate, setCandidate] = useState<{ taskId: string; content: string } | null>(null);
  const current = useRef(groupId);
  current.current = groupId;
  const sequence = useRef(0);
  const pendingRead = useRef<number | null>(null);
  const invalidateLoads = useCallback(() => {
    sequence.current++;
  }, []);
  const actionVisit = useRef({ groupId, active });
  if (actionVisit.current.groupId !== groupId || actionVisit.current.active !== active)
    actionVisit.current = { groupId, active };
  const mutationVisit = useRef<typeof actionVisit.current | null>(null);
  const retryAccepted = useRef(onRetryAccepted);
  retryAccepted.current = onRetryAccepted;
  const load = useCallback(
    async (afterMutation = false) => {
      if (!afterMutation && mutationVisit.current === actionVisit.current) return;
      if (pendingRead.current === sequence.current) return;
      const id = ++sequence.current;
      pendingRead.current = id;
      try {
        const response = await api.fetchSecretaryTasks(groupId);
        if (current.current !== groupId || id !== sequence.current) return;
        if (response.ok) {
          setTasks(response.result.tasks);
          setCoverage({
            deferred: response.result.deferred_sources || 0,
            held: response.result.held_sources || 0,
            invalid: response.result.invalid_sources || 0,
            unprocessed: response.result.unprocessed_document_sources || 0,
            configured: response.result.configured,
            ready: response.result.ready,
            readiness_code: response.result.readiness_code,
            readiness_error: response.result.readiness_error || "",
          });
          setLoadError("");
        } else setLoadError(response.error.message);
      } catch {
        if (current.current === groupId && id === sequence.current)
          setLoadError(t("voiceSettings.loadFailed"));
      } finally {
        if (pendingRead.current === id) pendingRead.current = null;
      }
    },
    [groupId, t],
  );
  useEffect(() => {
    sequence.current++;
    setTasks([]);
    setCoverage(EMPTY_COVERAGE);
    setFollowups({});
    setCandidate(null);
    setError("");
    setLoadError("");
  }, [groupId]);
  useEffect(() => {
    mutationVisit.current = null;
    setBusy("");
  }, [groupId, active]);
  useEffect(() => {
    if (!active || !groupId) return;
    void load();
    const timer = window.setInterval(() => void load(), 2000);
    return () => {
      window.clearInterval(timer);
      invalidateLoads();
    };
  }, [active, groupId, load, invalidateLoads]);
  const act = async (task: SecretaryTaskSummary, action: SecretaryTaskAction) => {
    if (
      action === "retry" &&
      task.phase === "unconfirmed" &&
      !window.confirm(t("voiceSettings.retryUnconfirmed"))
    )
      return;
    if (
      action === "handoff" &&
      !window.confirm(
        t("voiceSettings.confirmHandoff", {
          target: task.receipt?.output.handoff_target,
          text: task.receipt?.output.handoff_text,
        }),
      )
    )
      return;
    const gid = groupId;
    const visit = actionVisit.current;
    mutationVisit.current = visit;
    sequence.current++;
    setBusy(task.task_id);
    setError("");
    try {
      if (action === "candidate") {
        const result = await api.fetchSecretaryCandidate(gid, task.task_id);
        if (actionVisit.current !== visit) return;
        if (result.ok) setCandidate({ taskId: task.task_id, content: result.result.content });
        else setError(result.error.message);
      } else {
        const retryResult =
          action === "retry"
            ? await api.retrySecretaryTask(
                gid,
                task.task_id,
                followups[task.task_id] || "",
                task.phase === "unconfirmed",
              )
            : null;
        const result =
          retryResult ??
          (action === "cancel"
            ? await api.cancelSecretaryTask(gid, task.task_id)
            : await api.forwardSecretaryProposal(gid, task.task_id));
        if (actionVisit.current !== visit) return;
        if (!result.ok) setError(result.error.message);
        else {
          if (retryResult?.ok) retryAccepted.current?.(retryResult.result.task);
          setFollowups((previous) => ({ ...previous, [task.task_id]: "" }));
          await load(true);
        }
      }
    } catch {
      if (actionVisit.current === visit) setError(t("voiceSettings.actionFailed"));
    } finally {
      if (actionVisit.current === visit) {
        mutationVisit.current = null;
        setBusy("");
      }
    }
  };
  const setFollowup = (taskId: string, value: string) =>
    setFollowups((previous) => ({ ...previous, [taskId]: value }));
  const copyCandidate = (content: string) => {
    const visit = actionVisit.current;
    void copyTextToClipboard(content).then((copied) => {
      if (!copied && actionVisit.current === visit) setError(t("common:copyFailed"));
    });
  };
  return {
    tasks,
    coverage,
    error,
    loadError,
    busy,
    followups,
    setFollowup,
    candidate,
    copyCandidate,
    act,
  };
}

export type SecretaryTasksController = ReturnType<typeof useSecretaryTasks>;
