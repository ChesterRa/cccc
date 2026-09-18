import { formatRuntimeCommand } from "../components/modals/runtimeProfileControlsModel";
// Actor action helpers extracted from ActorTab-related logic.
import { useCallback, useRef, useState } from "react";
import { useGroupStore, useUIStore, useModalStore, useInboxStore, useFormStore } from "../stores";
import * as api from "../services/api";
import type { Actor, SupportedRuntime } from "../types";
import { formatCapabilityIdInput } from "../utils/capabilityAutoload";
import { beginActorAction, endActorAction } from "./actorActionInFlight";
import { resolveActorLifecycleRunning } from "./actorLifecycleAction";
import { useShallow } from "zustand/react/shallow";

function latestActorHasResumeFailure(groupId: string, actorId: string): boolean {
  if (useGroupStore.getState().selectedGroupId !== groupId) return false;
  const aid = String(actorId || "").trim();
  if (!aid) return false;
  const latest = useGroupStore
    .getState()
    .actors.find((item) => String(item.id || "").trim() === aid);
  return (
    String(latest?.runtime_session_status || "")
      .trim()
      .toLowerCase() === "resume_failed"
  );
}

export function useActorActions(groupId: string) {
  const { refreshActors, refreshGroups, loadGroup, clearStreamingEventsForActor } = useGroupStore(
    useShallow((s) => ({
      refreshActors: s.refreshActors,
      refreshGroups: s.refreshGroups,
      loadGroup: s.loadGroup,
      clearStreamingEventsForActor: s.clearStreamingEventsForActor,
    })),
  );
  const { changeActorBusy, setActiveTab, showError } = useUIStore(
    useShallow((s) => ({
      changeActorBusy: s.changeActorBusy,
      setActiveTab: s.setActiveTab,
      showError: s.showError,
    })),
  );
  const { openModal, setEditingActor } = useModalStore(
    useShallow((s) => ({ openModal: s.openModal, setEditingActor: s.setEditingActor })),
  );
  const { openInbox, setInboxMessages } = useInboxStore(
    useShallow((s) => ({ openInbox: s.openInbox, setInboxMessages: s.setInboxMessages })),
  );
  const {
    setEditActorRuntime,
    setEditActorCommand,
    setEditActorTitle,
    setEditActorCapabilityAutoloadText,
  } = useFormStore(
    useShallow((s) => ({
      setEditActorRuntime: s.setEditActorRuntime,
      setEditActorCommand: s.setEditActorCommand,
      setEditActorTitle: s.setEditActorTitle,
      setEditActorCapabilityAutoloadText: s.setEditActorCapabilityAutoloadText,
    })),
  );

  // Local state: terminal epoch is used to force a terminal re-mount.
  const [termEpochByActor, setTermEpochByActor] = useState<Record<string, number>>({});
  const actorActionInFlightRef = useRef<Set<string>>(new Set());

  // Start/stop actor
  const toggleActorEnabled = useCallback(
    async (actor: Actor, runningOverride?: boolean) => {
      if (!actor || !groupId) return;
      const isRunning = resolveActorLifecycleRunning(actor, runningOverride);
      const actionKey = JSON.stringify([groupId, actor.id]);
      if (!beginActorAction(actorActionInFlightRef, actionKey)) return;
      changeActorBusy(groupId, actor.id, 1);
      try {
        const resp = isRunning
          ? await api.stopActor(groupId, actor.id)
          : await api.startActor(groupId, actor.id);
        if (!resp.ok) {
          await Promise.all([refreshActors(groupId), refreshGroups()]);
          if (isRunning || !latestActorHasResumeFailure(groupId, actor.id)) {
            showError(`${resp.error.code}: ${resp.error.message}`);
          }
          return;
        }
        clearStreamingEventsForActor(actor.id, groupId);
        await Promise.all([refreshActors(groupId), refreshGroups()]);
      } finally {
        endActorAction(actorActionInFlightRef, actionKey);
        changeActorBusy(groupId, actor.id, -1);
      }
    },
    [
      groupId,
      changeActorBusy,
      showError,
      refreshActors,
      refreshGroups,
      clearStreamingEventsForActor,
    ],
  );

  // Restart actor
  const relaunchActor = useCallback(
    async (actor: Actor) => {
      if (!groupId || !actor) return;
      const actionKey = JSON.stringify([groupId, actor.id]);
      if (!beginActorAction(actorActionInFlightRef, actionKey)) return;
      changeActorBusy(groupId, actor.id, 1);
      try {
        const resp = await api.restartActor(groupId, actor.id);
        if (!resp.ok) {
          await Promise.all([refreshActors(groupId), refreshGroups()]);
          if (!latestActorHasResumeFailure(groupId, actor.id)) {
            showError(`${resp.error.code}: ${resp.error.message}`);
          }
        } else {
          await Promise.all([refreshActors(groupId), refreshGroups()]);
        }
        setTermEpochByActor((prev) => ({ ...prev, [actionKey]: (prev[actionKey] || 0) + 1 }));
      } finally {
        endActorAction(actorActionInFlightRef, actionKey);
        changeActorBusy(groupId, actor.id, -1);
      }
    },
    [groupId, changeActorBusy, showError, refreshActors, refreshGroups],
  );

  // Start a fresh runtime session with the actor's current settings.
  const startNewActorSession = useCallback(
    async (actor: Actor) => {
      if (!groupId || !actor) return;
      const actionKey = JSON.stringify([groupId, actor.id]);
      if (!beginActorAction(actorActionInFlightRef, actionKey)) return;
      changeActorBusy(groupId, actor.id, 1);
      try {
        const resp = await api.newActorSession(groupId, actor.id);
        if (!resp.ok) {
          await Promise.all([refreshActors(groupId), refreshGroups()]);
          showError(`${resp.error.code}: ${resp.error.message}`);
        } else {
          clearStreamingEventsForActor(actor.id, groupId);
          await Promise.all([refreshActors(groupId), refreshGroups()]);
        }
        setTermEpochByActor((prev) => ({ ...prev, [actionKey]: (prev[actionKey] || 0) + 1 }));
      } finally {
        endActorAction(actorActionInFlightRef, actionKey);
        changeActorBusy(groupId, actor.id, -1);
      }
    },
    [
      groupId,
      changeActorBusy,
      showError,
      refreshActors,
      refreshGroups,
      clearStreamingEventsForActor,
    ],
  );

  // Edit actor (initialize form state and open modal).
  const editActor = useCallback(
    (actor: Actor) => {
      if (!actor) return;
      // Initialize form state with actor's current values
      const runtime = String(actor.runtime || "").trim();
      setEditActorRuntime((runtime || "codex") as SupportedRuntime);
      setEditActorCommand(formatRuntimeCommand(actor.command));
      setEditActorTitle(actor.title || "");
      setEditActorCapabilityAutoloadText(formatCapabilityIdInput(actor.capability_autoload));
      setEditingActor(actor);
    },
    [
      setEditingActor,
      setEditActorRuntime,
      setEditActorCommand,
      setEditActorTitle,
      setEditActorCapabilityAutoloadText,
    ],
  );

  // Remove actor
  const removeActor = useCallback(
    async (actor: Actor, currentActiveTab: string) => {
      if (!actor || !groupId) return;
      if (!window.confirm(`Remove actor "${actor.title || actor.id}"?`)) return;
      changeActorBusy(groupId, actor.id, 1);
      try {
        const resp = await api.removeActor(groupId, actor.id);
        if (!resp.ok) {
          showError(`${resp.error.code}: ${resp.error.message}`);
          return;
        }
        clearStreamingEventsForActor(actor.id, groupId);
        if (
          useGroupStore.getState().selectedGroupId === groupId &&
          currentActiveTab === actor.id &&
          useUIStore.getState().activeTab === actor.id
        ) {
          setActiveTab("chat");
        }
        await Promise.all([refreshActors(groupId), refreshGroups()]);
        if (useGroupStore.getState().selectedGroupId === groupId) await loadGroup(groupId);
      } finally {
        changeActorBusy(groupId, actor.id, -1);
      }
    },
    [
      groupId,
      changeActorBusy,
      showError,
      refreshActors,
      refreshGroups,
      loadGroup,
      setActiveTab,
      clearStreamingEventsForActor,
    ],
  );

  // Open inbox modal
  const openActorInbox = useCallback(
    async (actor: Actor) => {
      if (!actor || !groupId) return;
      const target = { groupId, actorId: actor.id };
      changeActorBusy(groupId, actor.id, 1);
      try {
        openInbox(target);
        openModal("inbox");
        const resp = await api.fetchInbox(groupId, actor.id);
        if (useInboxStore.getState().inboxTarget !== target) return;
        if (!resp.ok) {
          showError(`${resp.error.code}: ${resp.error.message}`);
          return;
        }
        setInboxMessages(target, resp.result.messages || []);
      } finally {
        changeActorBusy(groupId, actor.id, -1);
      }
    },
    [groupId, changeActorBusy, showError, openInbox, setInboxMessages, openModal],
  );

  // Get actor termEpoch
  const getTermEpoch = useCallback(
    (actorId: string) => termEpochByActor[JSON.stringify([groupId, actorId])] || 0,
    [groupId, termEpochByActor],
  );

  return {
    termEpochByActor,
    getTermEpoch,
    toggleActorEnabled,
    relaunchActor,
    startNewActorSession,
    editActor,
    removeActor,
    openActorInbox,
  };
}
