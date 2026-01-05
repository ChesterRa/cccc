// Actor 操作 hook - 抽离 ActorTab 相关的所有操作逻辑
import { useCallback, useState } from "react";
import { useGroupStore, useUIStore, useModalStore, useInboxStore } from "../stores";
import * as api from "../services/api";
import type { Actor } from "../types";

export function useActorActions(groupId: string) {
  const { refreshActors, loadGroup } = useGroupStore();
  const { setBusy, setActiveTab, showError } = useUIStore();
  const { openModal, setEditingActor } = useModalStore();
  const { setInboxActorId, setInboxMessages } = useInboxStore();

  // 本地状态：terminal epoch 用于强制重新挂载终端
  const [termEpochByActor, setTermEpochByActor] = useState<Record<string, number>>({});

  // 启动/停止 actor
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

  // 重启 actor
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

  // 编辑 actor (表单状态由 Modal 组件自己管理)
  const editActor = useCallback(
    (actor: Actor) => {
      if (!actor) return;
      const isRunning = actor.running ?? actor.enabled ?? false;
      if (isRunning) {
        showError("Stop the actor before editing.");
        return;
      }
      setEditingActor(actor);
    },
    [setEditingActor, showError]
  );

  // 删除 actor
  const removeActor = useCallback(
    async (actor: Actor, currentActiveTab: string) => {
      if (!actor || !groupId) return;
      if (!window.confirm(`Remove actor ${actor.id}?`)) return;
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

  // 打开 inbox
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

  // 获取 actor 的 termEpoch
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
