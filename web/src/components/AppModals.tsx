// AppModals - 集中渲染所有 Modal 组件
import { ContextModal } from "./ContextModal";
import { SettingsModal } from "./SettingsModal";
import { SearchModal } from "./SearchModal";
import { MobileMenuSheet } from "./layout/MobileMenuSheet";
import { AddActorModal } from "./modals/AddActorModal";
import { CreateGroupModal } from "./modals/CreateGroupModal";
import { EditActorModal } from "./modals/EditActorModal";
import { GroupEditModal } from "./modals/GroupEditModal";
import { InboxModal } from "./modals/InboxModal";
import { RecipientsModal } from "./modals/RecipientsModal";
import {
  useGroupStore,
  useUIStore,
  useModalStore,
  useInboxStore,
  useFormStore,
} from "../stores";
import * as api from "../services/api";
import { RUNTIME_INFO, LedgerEvent, GroupSettings } from "../types";

interface AppModalsProps {
  isDark: boolean;
  composerRef: React.RefObject<HTMLTextAreaElement>;
  messageMeta: {
    toLabel: string;
    entries: readonly (readonly [string, boolean])[];
  } | null;
  onStartReply: (ev: LedgerEvent) => void;
  onThemeToggle: () => void;
  onStartGroup: () => Promise<void>;
  onStopGroup: () => Promise<void>;
  onSetGroupState: (state: "active" | "idle" | "paused") => Promise<void>;
  fetchContext: (groupId: string) => Promise<void>;
}

export function AppModals({
  isDark,
  composerRef,
  messageMeta,
  onStartReply,
  onThemeToggle,
  onStartGroup,
  onStopGroup,
  onSetGroupState,
  fetchContext,
}: AppModalsProps) {
  // Stores
  const {
    selectedGroupId,
    groupDoc,
    actors,
    groupContext,
    groupSettings,
    runtimes,
    setSelectedGroupId,
    setGroupDoc,
    setGroupContext,
    setGroupSettings,
    refreshGroups,
    refreshActors,
    loadGroup,
  } = useGroupStore();

  const {
    busy,
    isSmallScreen,
    setBusy,
    showError,
    setActiveTab,
  } = useUIStore();

  const {
    modals,
    recipientsEventId: _recipientsEventId,
    editingActor,
    openModal,
    closeModal,
    setRecipientsModal,
    setEditingActor,
  } = useModalStore();

  const { inboxActorId, inboxMessages, setInboxMessages } = useInboxStore();

  const {
    editGroupTitle,
    editGroupTopic,
    setEditGroupTitle,
    setEditGroupTopic,
    editActorRuntime,
    editActorCommand,
    editActorTitle,
    setEditActorRuntime,
    setEditActorCommand,
    newActorId,
    newActorRole,
    newActorRuntime,
    newActorCommand,
    showAdvancedActor,
    addActorError,
    setNewActorId,
    setNewActorRole,
    setNewActorRuntime,
    setNewActorCommand,
    setShowAdvancedActor,
    setAddActorError,
    resetAddActorForm,
    createGroupPath,
    createGroupName,
    dirItems,
    dirSuggestions,
    currentDir,
    parentDir,
    showDirBrowser,
    setCreateGroupPath,
    setCreateGroupName,
    setDirItems,
    setCurrentDir,
    setParentDir,
    setShowDirBrowser,
    resetCreateGroupForm,
  } = useFormStore();

  // Computed
  const selectedGroupRunning = useGroupStore(
    (s) => s.groups.find((g) => String(g.group_id || "") === s.selectedGroupId)?.running ?? false
  );
  const hasForeman = actors.some((a) => a.role === "foreman");

  // Handlers
  const handleUpdateVision = async (vision: string) => {
    if (!selectedGroupId) return;
    setBusy("context-update");
    try {
      const resp = await api.updateVision(selectedGroupId, vision);
      if (!resp.ok) showError(`${resp.error.code}: ${resp.error.message}`);
      void fetchContext(selectedGroupId);
    } finally {
      setBusy("");
    }
  };

  const handleUpdateSketch = async (sketch: string) => {
    if (!selectedGroupId) return;
    setBusy("context-update");
    try {
      const resp = await api.updateSketch(selectedGroupId, sketch);
      if (!resp.ok) showError(`${resp.error.code}: ${resp.error.message}`);
      void fetchContext(selectedGroupId);
    } finally {
      setBusy("");
    }
  };

  const handleUpdateSettings = async (settings: Partial<GroupSettings>) => {
    if (!selectedGroupId) return;
    setBusy("settings-update");
    try {
      const resp = await api.updateSettings(selectedGroupId, settings);
      if (!resp.ok) showError(`${resp.error.code}: ${resp.error.message}`);
      const settingsResp = await api.fetchSettings(selectedGroupId);
      if (settingsResp.ok && settingsResp.result.settings) {
        setGroupSettings(settingsResp.result.settings);
      }
    } finally {
      setBusy("");
    }
  };

  const handleMarkAllRead = async () => {
    if (!selectedGroupId || !inboxActorId) return;
    const last = inboxMessages.length ? inboxMessages[inboxMessages.length - 1] : null;
    const eventId = last?.id ? String(last.id) : "";
    if (!eventId) return;
    setBusy(`inbox-read:${inboxActorId}`);
    try {
      const resp = await api.markInboxRead(selectedGroupId, inboxActorId, eventId);
      if (!resp.ok) {
        showError(`${resp.error.code}: ${resp.error.message}`);
        return;
      }
      const inboxResp = await api.fetchInbox(selectedGroupId, inboxActorId);
      if (inboxResp.ok) {
        setInboxMessages(inboxResp.result.messages || []);
      }
    } finally {
      setBusy("");
    }
  };

  const handleSaveGroupEdit = async () => {
    if (!selectedGroupId) return;
    setBusy("group-update");
    try {
      const resp = await api.updateGroup(selectedGroupId, editGroupTitle, editGroupTopic);
      if (!resp.ok) {
        showError(`${resp.error.code}: ${resp.error.message}`);
        return;
      }
      closeModal("groupEdit");
      await refreshGroups();
      await loadGroup(selectedGroupId);
    } finally {
      setBusy("");
    }
  };

  const handleDeleteGroup = async () => {
    if (!selectedGroupId) return;
    if (!window.confirm(`Delete group "${groupDoc?.title || selectedGroupId}"?`)) return;
    setBusy("group-delete");
    try {
      const resp = await api.deleteGroup(selectedGroupId);
      if (!resp.ok) {
        showError(`${resp.error.code}: ${resp.error.message}`);
        return;
      }
      setSelectedGroupId("");
      setGroupDoc(null);
      useGroupStore.getState().setEvents([]);
      useGroupStore.getState().setActors([]);
      setGroupContext(null);
      setGroupSettings(null);
      await refreshGroups();
    } finally {
      setBusy("");
    }
  };

  const handleSaveEditActor = async () => {
    if (!selectedGroupId || !editingActor) return;
    setBusy("actor-update");
    try {
      const resp = await api.updateActor(
        selectedGroupId,
        editingActor.id,
        editActorRuntime,
        editActorCommand,
        editActorTitle
      );
      if (!resp.ok) {
        showError(`${resp.error.code}: ${resp.error.message}`);
        return;
      }
      setEditingActor(null);
      await refreshActors();
    } finally {
      setBusy("");
    }
  };

  const handleFetchDirContents = async (path: string) => {
    setShowDirBrowser(true);
    const resp = await api.fetchDirContents(path);
    if (resp.ok) {
      setDirItems(resp.result.items || []);
      setCurrentDir(resp.result.path || path);
      setParentDir(resp.result.parent || null);
    } else {
      showError(resp.error?.message || "Failed to list directory");
    }
  };

  const handleCreateGroup = async () => {
    const path = createGroupPath.trim();
    if (!path) return;
    const dirName = path.split("/").filter(Boolean).pop() || "working-group";
    const title = createGroupName.trim() || dirName;
    setBusy("create");
    try {
      const resp = await api.createGroup(title);
      if (!resp.ok) {
        showError(`${resp.error.code}: ${resp.error.message}`);
        return;
      }
      const groupId = resp.result.group_id;
      const attachResp = await api.attachScope(groupId, path);
      if (!attachResp.ok) {
        showError(`Created group but failed to attach: ${attachResp.error.message}`);
      }
      resetCreateGroupForm();
      closeModal("createGroup");
      await refreshGroups();
      setSelectedGroupId(groupId);
    } finally {
      setBusy("");
    }
  };

  const handleAddActor = async () => {
    if (!selectedGroupId) return;
    const actorId = newActorId.trim();
    setBusy("actor-add");
    setAddActorError("");
    try {
      const resp = await api.addActor(
        selectedGroupId,
        actorId,
        newActorRole,
        newActorRuntime,
        newActorCommand
      );
      if (!resp.ok) {
        setAddActorError(resp.error?.message || "Failed to add agent");
        return;
      }
      closeModal("addActor");
      resetAddActorForm();
      await refreshActors();
    } finally {
      setBusy("");
    }
  };

  // Computed for AddActorModal
  const suggestedActorId = (() => {
    const prefix = newActorRuntime;
    const existing = new Set(actors.map((a) => String(a.id || "")));
    for (let i = 1; i <= 999; i++) {
      const candidate = `${prefix}-${i}`;
      if (!existing.has(candidate)) return candidate;
    }
    return `${prefix}-${Date.now()}`;
  })();

  const canAddActor = (() => {
    if (busy === "actor-add") return false;
    const rtInfo = runtimes.find((r) => r.name === newActorRuntime);
    const available = rtInfo?.available ?? false;
    if (newActorRuntime === "custom" && !newActorCommand.trim()) return false;
    if (!available && !newActorCommand.trim()) return false;
    return true;
  })();

  const addActorDisabledReason = (() => {
    if (busy === "actor-add") return "";
    const rtInfo = runtimes.find((r) => r.name === newActorRuntime);
    const available = rtInfo?.available ?? false;
    if (newActorRuntime === "custom" && !newActorCommand.trim()) {
      return "Custom runtime requires a command.";
    }
    if (!available && !newActorCommand.trim()) {
      return `${RUNTIME_INFO[newActorRuntime]?.label || newActorRuntime} is not installed.`;
    }
    return "";
  })();

  return (
    <>
      <MobileMenuSheet
        isOpen={modals.mobileMenu}
        isDark={isDark}
        selectedGroupId={selectedGroupId}
        groupDoc={groupDoc}
        selectedGroupRunning={selectedGroupRunning}
        actors={actors}
        busy={busy}
        onClose={() => closeModal("mobileMenu")}
        onToggleTheme={onThemeToggle}
        onOpenSearch={() => openModal("search")}
        onOpenContext={() => {
          if (selectedGroupId) void fetchContext(selectedGroupId);
          openModal("context");
        }}
        onOpenSettings={() => openModal("settings")}
        onOpenGroupEdit={() => {
          if (groupDoc) {
            setEditGroupTitle(groupDoc.title || "");
            setEditGroupTopic(groupDoc.topic || "");
            openModal("groupEdit");
          }
        }}
        onStartGroup={onStartGroup}
        onStopGroup={onStopGroup}
        onSetGroupState={onSetGroupState}
      />

      <SearchModal
        isOpen={modals.search}
        onClose={() => closeModal("search")}
        groupId={selectedGroupId}
        actors={actors}
        isDark={isDark}
        onReply={(ev) => {
          onStartReply(ev);
          setActiveTab("chat");
          closeModal("search");
          window.setTimeout(() => composerRef.current?.focus(), 0);
        }}
      />

      <ContextModal
        isOpen={modals.context}
        onClose={() => closeModal("context")}
        groupId={selectedGroupId}
        context={groupContext}
        onUpdateVision={handleUpdateVision}
        onUpdateSketch={handleUpdateSketch}
        busy={busy === "context-update"}
        isDark={isDark}
      />

      <SettingsModal
        isOpen={modals.settings}
        onClose={() => closeModal("settings")}
        settings={groupSettings}
        onUpdateSettings={handleUpdateSettings}
        busy={busy.startsWith("settings")}
        isDark={isDark}
        groupId={selectedGroupId}
      />

      <RecipientsModal
        isOpen={!!messageMeta}
        isDark={isDark}
        isSmallScreen={isSmallScreen}
        toLabel={messageMeta?.toLabel || ""}
        entries={(messageMeta?.entries || []) as [string, boolean][]}
        onClose={() => setRecipientsModal(null)}
      />

      <InboxModal
        isOpen={modals.inbox}
        isDark={isDark}
        actorId={inboxActorId}
        messages={inboxMessages}
        busy={busy}
        onClose={() => closeModal("inbox")}
        onMarkAllRead={handleMarkAllRead}
      />

      <GroupEditModal
        isOpen={modals.groupEdit}
        isDark={isDark}
        busy={busy}
        title={editGroupTitle}
        topic={editGroupTopic}
        onChangeTitle={setEditGroupTitle}
        onChangeTopic={setEditGroupTopic}
        onSave={handleSaveGroupEdit}
        onCancel={() => closeModal("groupEdit")}
        onDelete={handleDeleteGroup}
      />

      <EditActorModal
        isOpen={!!editingActor}
        isDark={isDark}
        busy={busy}
        actorId={editingActor?.id || ""}
        runtimes={runtimes}
        runtime={editActorRuntime}
        onChangeRuntime={setEditActorRuntime}
        command={editActorCommand}
        onChangeCommand={setEditActorCommand}
        onSave={handleSaveEditActor}
        onCancel={() => setEditingActor(null)}
      />

      <CreateGroupModal
        isOpen={modals.createGroup}
        isDark={isDark}
        busy={busy}
        dirSuggestions={dirSuggestions}
        dirItems={dirItems}
        currentDir={currentDir}
        parentDir={parentDir}
        showDirBrowser={showDirBrowser}
        createGroupPath={createGroupPath}
        setCreateGroupPath={setCreateGroupPath}
        createGroupName={createGroupName}
        setCreateGroupName={setCreateGroupName}
        onFetchDirContents={handleFetchDirContents}
        onCreateGroup={handleCreateGroup}
        onClose={() => closeModal("createGroup")}
        onCancelAndReset={() => {
          closeModal("createGroup");
          resetCreateGroupForm();
        }}
      />

      <AddActorModal
        isOpen={modals.addActor}
        isDark={isDark}
        busy={busy}
        hasForeman={hasForeman}
        runtimes={runtimes}
        suggestedActorId={suggestedActorId}
        newActorId={newActorId}
        setNewActorId={setNewActorId}
        newActorRole={newActorRole}
        setNewActorRole={setNewActorRole}
        newActorRuntime={newActorRuntime}
        setNewActorRuntime={setNewActorRuntime}
        newActorCommand={newActorCommand}
        setNewActorCommand={setNewActorCommand}
        showAdvancedActor={showAdvancedActor}
        setShowAdvancedActor={setShowAdvancedActor}
        addActorError={addActorError}
        setAddActorError={setAddActorError}
        canAddActor={canAddActor}
        addActorDisabledReason={addActorDisabledReason}
        onAddActor={handleAddActor}
        onClose={() => closeModal("addActor")}
        onCancelAndReset={() => {
          closeModal("addActor");
          resetAddActorForm();
        }}
      />
    </>
  );
}
