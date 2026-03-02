// Actor action helpers extracted from ActorTab-related logic.
import { useCallback, useState } from "react";
import { useGroupStore, useUIStore, useModalStore, useInboxStore, useFormStore } from "../stores";
import * as api from "../services/api";
import type { Actor, SupportedRuntime } from "../types";
import { formatCapabilityIdInput } from "../utils/capabilityAutoload";

export function useActorActions(groupId: string) {
  const { refreshActors, loadGroup } = useGroupStore();
  const { setBusy, setActiveTab, showError } = useUIStore();
  const { openModal, setEditingActor } = useModalStore();
  const { setInboxActorId, setInboxMessages } = useInboxStore();
  const { setEditActorRuntime, setEditActorCommand, setEditActorTitle, setEditActorCapabilityAutoloadText } =
    useFormStore();

  // Local state: terminal epoch is used to force a terminal re-mount.
  const [termEpochByActor, setTermEpochByActor] = useState<Record<string, number>>({});

  // Start/stop actor
  const toggleActorEnabled = useCallback(
    async (actor: Actor) => {
      if (!actor || !groupId) return;
      const isRunning = actor.running ?? actor.enabled ?? false;
      setBusy(`actor-${isRunning ? "stop" : "start"}:${actor.id}`);
      try {
        const resp = isRunning
          ? await api.stopActor(groupId, actor.id)
          : await api.startActor(groupId, actor.id);
        if (!resp.ok) {
          showError(`${resp.error.code}: ${resp.error.message}`);
          return;
        }
        await refreshActors();
      } finally {
        setBusy("");
      }
    },
    [groupId, setBusy, showError, refreshActors]
  );

  // Restart actor
  const relaunchActor = useCallback(
    async (actor: Actor) => {
      if (!groupId || !actor) return;
      setBusy(`actor-relaunch:${actor.id}`);
      try {
        const resp = await api.restartActor(groupId, actor.id);
        if (!resp.ok) {
          showError(`${resp.error.code}: ${resp.error.message}`);
        }
        await refreshActors();
        setTermEpochByActor((prev) => ({
          ...prev,
          [actor.id]: (prev[actor.id] || 0) + 1,
        }));
      } finally {
        setBusy("");
      }
    },
    [groupId, setBusy, showError, refreshActors]
  );

  // Edit actor (initialize form state and open modal).
  const editActor = useCallback(
    (actor: Actor) => {
      if (!actor) return;
      // Initialize form state with actor's current values
      const runtime = String(actor.runtime || "").trim();
      setEditActorRuntime((runtime || "codex") as SupportedRuntime);
      setEditActorCommand(Array.isArray(actor.command) ? actor.command.join(" ") : "");
      setEditActorTitle(actor.title || "");
      setEditActorCapabilityAutoloadText(formatCapabilityIdInput(actor.capability_autoload));
      setEditingActor(actor);
    },
    [setEditingActor, setEditActorRuntime, setEditActorCommand, setEditActorTitle, setEditActorCapabilityAutoloadText]
  );

  // Remove actor
  const removeActor = useCallback(
    async (actor: Actor, currentActiveTab: string) => {
      if (!actor || !groupId) return;
      if (!window.confirm(`Remove actor "${actor.title || actor.id}"?`)) return;
      setBusy(`actor-remove:${actor.id}`);
      try {
        const resp = await api.removeActor(groupId, actor.id);
        if (!resp.ok) {
          showError(`${resp.error.code}: ${resp.error.message}`);
          return;
        }
        if (currentActiveTab === actor.id) {
          setActiveTab("chat");
        }
        await refreshActors();
        await loadGroup(groupId);
      } finally {
        setBusy("");
      }
    },
    [groupId, setBusy, showError, refreshActors, loadGroup, setActiveTab]
  );

  // Open inbox modal
  const openActorInbox = useCallback(
    async (actor: Actor) => {
      if (!actor || !groupId) return;
      setBusy(`inbox:${actor.id}`);
      try {
        setInboxActorId(actor.id);
        setInboxMessages([]);
        openModal("inbox");
        const resp = await api.fetchInbox(groupId, actor.id);
        if (!resp.ok) {
          showError(`${resp.error.code}: ${resp.error.message}`);
          return;
        }
        setInboxMessages(resp.result.messages || []);
      } finally {
        setBusy("");
      }
    },
    [groupId, setBusy, showError, setInboxActorId, setInboxMessages, openModal]
  );

  // Get actor termEpoch
  const getTermEpoch = useCallback(
    (actorId: string) => termEpochByActor[actorId] || 0,
    [termEpochByActor]
  );

  return {
    termEpochByActor,
    getTermEpoch,
    toggleActorEnabled,
    relaunchActor,
    editActor,
    removeActor,
    openActorInbox,
  };
}
