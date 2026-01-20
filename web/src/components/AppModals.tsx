// AppModals renders all modal components in one place.
import { useEffect, useMemo, useState } from "react";
import { ContextModal } from "./ContextModal";
import { SettingsModal } from "./SettingsModal";
import { SearchModal } from "./SearchModal";
import { MobileMenuSheet } from "./layout/MobileMenuSheet";
import { AddActorModal } from "./modals/AddActorModal";
import { CreateGroupModal } from "./modals/CreateGroupModal";
import { EditActorModal } from "./modals/EditActorModal";
import { GroupEditModal } from "./modals/GroupEditModal";
import { InboxModal } from "./modals/InboxModal";
import { RelayMessageModal } from "./modals/RelayMessageModal";
import { RecipientsModal } from "./modals/RecipientsModal";
import {
  useGroupStore,
  useUIStore,
  useModalStore,
  useInboxStore,
  useFormStore,
} from "../stores";
import * as api from "../services/api";
import { RUNTIME_INFO, LedgerEvent, GroupSettings, ChatMessageData } from "../types";

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
    groups,
    selectedGroupId,
    groupDoc,
    events,
    chatWindow,
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
    openChatWindow,
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
    relayEventId,
    relaySource,
    editingActor,
    openModal,
    closeModal,
    setRecipientsModal,
    setRelayModal,
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
    setEditActorTitle,
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
    createGroupTemplateFile,
    createWorktree,
    baseBranch,
    branches,
    loadingBranches,
    dirItems,
    dirSuggestions,
    currentDir,
    parentDir,
    showDirBrowser,
    setCreateGroupPath,
    setCreateGroupName,
    setCreateGroupTemplateFile,
    setCreateWorktree,
    setBaseBranch,
    setBranches,
    setLoadingBranches,
    setDirItems,
    setCurrentDir,
    setParentDir,
    setShowDirBrowser,
    resetCreateGroupForm,
  } = useFormStore();

  const [createTemplatePreview, setCreateTemplatePreview] = useState<any | null>(null);
  const [createTemplateScopeRoot, setCreateTemplateScopeRoot] = useState("");
  const [createTemplatePromptOverwriteFiles, setCreateTemplatePromptOverwriteFiles] = useState<string[]>([]);
  const [createTemplateError, setCreateTemplateError] = useState("");
  const [createTemplateBusy, setCreateTemplateBusy] = useState(false);

  const detectPromptOverwriteFiles = async (path: string): Promise<{ scopeRoot: string; files: string[] }> => {
    const p = String(path || "").trim();
    if (!p) return { scopeRoot: "", files: [] };
    try {
      let root = p;
      try {
        const rootResp = await api.resolveScopeRoot(p);
        if (rootResp.ok && rootResp.result?.scope_root) {
          root = String(rootResp.result.scope_root || "").trim() || p;
        }
      } catch {
        // ignore
      }

      const resp = await api.fetchDirContents(root);
      if (!resp.ok) return { scopeRoot: root, files: [] };
      const items = Array.isArray(resp.result?.items) ? resp.result.items : [];
      const names = new Set(items.map((it: any) => String(it?.name || "").trim()).filter((s: string) => s));
      const wanted = ["CCCC_PREAMBLE.md", "CCCC_HELP.md", "CCCC_STANDUP.md"];
      return { scopeRoot: root, files: wanted.filter((n) => names.has(n)) };
    } catch {
      return { scopeRoot: "", files: [] };
    }
  };

  useEffect(() => {
    if (!modals.createGroup) return;
    if (!createGroupTemplateFile || !createGroupPath.trim()) {
      setCreateTemplateScopeRoot("");
      setCreateTemplatePromptOverwriteFiles([]);
      return;
    }
    const t = window.setTimeout(() => {
      void (async () => {
        const res = await detectPromptOverwriteFiles(createGroupPath);
        setCreateTemplateScopeRoot(res.scopeRoot);
        setCreateTemplatePromptOverwriteFiles(res.files);
      })();
    }, 250);
    return () => window.clearTimeout(t);
  }, [modals.createGroup, createGroupTemplateFile, createGroupPath]);

  // Fetch branches when worktree option is enabled and path is set
  useEffect(() => {
    if (!modals.createGroup || !createWorktree || !createGroupPath.trim()) {
      setBranches([]);
      return;
    }
    const t = window.setTimeout(() => {
      void (async () => {
        setLoadingBranches(true);
        try {
          const resp = await api.fetchBranches(createGroupPath);
          if (resp.ok && resp.result?.branches) {
            const branchList = resp.result.branches;
            setBranches(branchList);
            // Auto-select current branch if available (current branch is valid base even if in_use)
            const currentBranch = branchList.find(b => b.is_current);
            if (currentBranch) {
              setBaseBranch(currentBranch.name);
            }
          } else {
            setBranches([]);
          }
        } catch {
          setBranches([]);
        } finally {
          setLoadingBranches(false);
        }
      })();
    }, 300);
    return () => window.clearTimeout(t);
  }, [modals.createGroup, createWorktree, createGroupPath]);

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

  const handleSaveEditActorAndRestart = async () => {
    if (!selectedGroupId || !editingActor) return;
    const label = editingActor.title || editingActor.id;
    if (!window.confirm(`Save changes and restart "${label}" now? This will interrupt any running work.`)) return;
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

      const restartResp = await api.restartActor(selectedGroupId, editingActor.id);
      if (!restartResp.ok) {
        showError(`${restartResp.error.code}: ${restartResp.error.message}`);
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

  const handleSelectCreateGroupTemplate = async (file: File | null) => {
    setCreateGroupTemplateFile(file);
    setCreateTemplatePreview(null);
    setCreateTemplateError("");
    setCreateTemplateScopeRoot("");
    setCreateTemplatePromptOverwriteFiles([]);
    if (!file) return;

    setCreateTemplateBusy(true);
    try {
      const resp = await api.previewTemplate(file);
      if (!resp.ok) {
        setCreateTemplateError(resp.error?.message || "Invalid template");
        return;
      }
      setCreateTemplatePreview(resp.result?.template || null);
      if (createGroupPath.trim()) {
        const res = await detectPromptOverwriteFiles(createGroupPath);
        setCreateTemplateScopeRoot(res.scopeRoot);
        setCreateTemplatePromptOverwriteFiles(res.files);
      }
    } catch {
      setCreateTemplateError("Failed to load template");
    } finally {
      setCreateTemplateBusy(false);
    }
  };

  const handleCreateGroup = async () => {
    const path = createGroupPath.trim();
    if (!path) return;
    const dirName = path.split("/").filter(Boolean).pop() || "working-group";
    const title = createGroupName.trim() || dirName;
    setBusy("create");
    try {
      let groupId = "";

      if (createGroupTemplateFile) {
        const overwrite = await detectPromptOverwriteFiles(path);
        if (overwrite.files.length > 0) {
          const ok = window.confirm(
            `This will modify repo prompt files (create/overwrite/delete):\n\n- ${overwrite.files.join("\n- ")}${
              overwrite.scopeRoot ? `\n\nProject root:\n- ${overwrite.scopeRoot}` : ""
            }\n\nContinue?`
          );
          if (!ok) return;
        }
        const resp = await api.createGroupFromTemplate(path, title, "", createGroupTemplateFile);
        if (!resp.ok) {
          if (resp.error?.code === "scope_already_attached" && (resp.error as any)?.details?.group_id) {
            const existing = String((resp.error as any).details.group_id || "");
            if (existing) {
              showError("This directory already has a working group. Opening it instead.");
              closeModal("createGroup");
              resetCreateGroupForm();
              setCreateTemplatePreview(null);
              setCreateTemplateError("");
              setCreateTemplateBusy(false);
              setCreateTemplatePromptOverwriteFiles([]);
              setSelectedGroupId(existing);
              return;
            }
          }
          showError(`${resp.error.code}: ${resp.error.message}`);
          return;
        }
        groupId = resp.result.group_id;
      } else {
        const resp = await api.createGroup(title);
        if (!resp.ok) {
          showError(`${resp.error.code}: ${resp.error.message}`);
          return;
        }
        groupId = resp.result.group_id;
        const attachResp = await api.attachScope(groupId, path, {
          createWorktree,
          baseBranch,
        });
        if (!attachResp.ok) {
          showError(`Created group but failed to attach: ${attachResp.error.message}`);
        }
      }

      resetCreateGroupForm();
      setCreateTemplatePreview(null);
      setCreateTemplateError("");
      setCreateTemplateBusy(false);
      setCreateTemplatePromptOverwriteFiles([]);
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

  const relaySourceEvent = useMemo(() => {
    const eid = String(relayEventId || "").trim();
    if (!eid) return null;
    const fromWindow =
      chatWindow && String(chatWindow.groupId || "") === String(selectedGroupId || "")
        ? (chatWindow.events || []).find((ev) => String(ev.id || "") === eid) || null
        : null;
    if (fromWindow) return fromWindow;
    return (events || []).find((ev) => String(ev.id || "") === eid) || null;
  }, [chatWindow, events, relayEventId, selectedGroupId]);

  const handleRelayMessage = async (dstGroupId: string, toTokens: string[], note: string) => {
    const src = relaySourceEvent;
    const srcGroupId = String(selectedGroupId || "").trim();
    const dstGroup = String(dstGroupId || "").trim();
    const srcEventId = src?.id ? String(src.id) : "";
    if (!srcGroupId || !srcEventId) return;
    if (!dstGroup) return;
    if (dstGroup === srcGroupId) {
      showError("Destination group must be different from the source group.");
      return;
    }

    const d = src.data as ChatMessageData | undefined;
    const srcText = typeof d?.text === "string" ? d.text : "";
    const noteText = String(note || "").trim();
    const relayText = (noteText ? noteText + "\n\n" : "") + String(srcText || "");
    if (!relayText.trim()) {
      showError("Relay message text is empty.");
      return;
    }

    const to = (toTokens || []).map((t) => String(t || "").trim()).filter((t) => t);

    setBusy("relay");
    try {
      const resp = await api.relayMessage(dstGroup, relayText, to, { groupId: srcGroupId, eventId: srcEventId });
      if (!resp.ok) {
        showError(`${resp.error.code}: ${resp.error.message}`);
        return;
      }
      setRelayModal(null);
      await refreshGroups();
    } finally {
      setBusy("");
    }
  };

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

      {modals.relay && relayEventId ? (
        <RelayMessageModal
          key={`${selectedGroupId}:${relayEventId}`}
          isOpen={true}
          isDark={isDark}
          busy={busy === "relay"}
          srcGroupId={selectedGroupId}
          srcEvent={relaySourceEvent}
          groups={groups}
          onCancel={() => setRelayModal(null)}
          onSubmit={(dstGroupId, to, note) => void handleRelayMessage(dstGroupId, to, note)}
        />
      ) : null}

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
        onJumpToMessage={(eventId) => {
          const gid = String(selectedGroupId || "").trim();
          const eid = String(eventId || "").trim();
          if (!gid || !eid) return;
          setActiveTab("chat");
          closeModal("search");
          const url = new URL(window.location.href);
          url.searchParams.set("group", gid);
          url.searchParams.set("event", eid);
          url.searchParams.set("tab", "chat");
          window.history.replaceState({}, "", url.pathname + "?" + url.searchParams.toString());
          void openChatWindow(gid, eid);
        }}
      />

      <ContextModal
        isOpen={modals.context}
        onClose={() => closeModal("context")}
        groupId={selectedGroupId}
        context={groupContext}
        onRefreshContext={async () => {
          if (selectedGroupId) await fetchContext(selectedGroupId);
        }}
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
        groupDoc={groupDoc}
      />

      <RecipientsModal
        isOpen={!!messageMeta}
        isDark={isDark}
        isSmallScreen={isSmallScreen}
        toLabel={messageMeta?.toLabel || ""}
        statusKind={messageMeta?.statusKind || "read"}
        entries={(messageMeta?.entries || []) as [string, boolean][]}
        onClose={() => setRecipientsModal(null)}
      />

      <InboxModal
        isOpen={modals.inbox}
        isDark={isDark}
        actorId={inboxActorId}
        actors={actors}
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
        isRunning={!!(editingActor && (editingActor.running ?? editingActor.enabled ?? false))}
        runtimes={runtimes}
        runtime={editActorRuntime}
        onChangeRuntime={setEditActorRuntime}
        command={editActorCommand}
        onChangeCommand={setEditActorCommand}
        title={editActorTitle}
        onChangeTitle={setEditActorTitle}
        onSave={handleSaveEditActor}
        onSaveAndRestart={handleSaveEditActorAndRestart}
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
        createGroupTemplateFile={createGroupTemplateFile}
        templatePreview={createTemplatePreview}
        scopeRoot={createTemplateScopeRoot}
        promptOverwriteFiles={createTemplatePromptOverwriteFiles}
        templateError={createTemplateError}
        templateBusy={createTemplateBusy}
        onSelectTemplate={handleSelectCreateGroupTemplate}
        createWorktree={createWorktree}
        setCreateWorktree={setCreateWorktree}
        baseBranch={baseBranch}
        setBaseBranch={setBaseBranch}
        branches={branches}
        loadingBranches={loadingBranches}
        onFetchDirContents={handleFetchDirContents}
        onCreateGroup={handleCreateGroup}
        onClose={() => closeModal("createGroup")}
        onCancelAndReset={() => {
          closeModal("createGroup");
          resetCreateGroupForm();
          setCreateTemplatePreview(null);
          setCreateTemplateError("");
          setCreateTemplateBusy(false);
          setCreateTemplatePromptOverwriteFiles([]);
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
