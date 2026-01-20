// Form state store for modals.
import { create } from "zustand";
import type { DirItem, DirSuggestion, SupportedRuntime } from "../types";
import type { BranchInfo } from "../services/api";

interface FormState {
  // Edit Group
  editGroupTitle: string;
  editGroupTopic: string;

  // Edit Actor
  editActorRuntime: SupportedRuntime;
  editActorCommand: string;
  editActorTitle: string;

  // Add Actor
  newActorId: string;
  newActorRole: "peer" | "foreman";
  newActorRuntime: SupportedRuntime;
  newActorCommand: string;
  showAdvancedActor: boolean;
  addActorError: string;

  // Create Group
  createGroupPath: string;
  createGroupName: string;
  createGroupTemplateFile: File | null;
  createWorktree: boolean;
  baseBranch: string;  // Base branch for auto-branch mode (cccc/<group_id>)
  branches: BranchInfo[];
  loadingBranches: boolean;
  dirItems: DirItem[];
  dirSuggestions: DirSuggestion[];
  currentDir: string;
  parentDir: string | null;
  showDirBrowser: boolean;

  // Actions - Edit Group
  setEditGroupTitle: (v: string) => void;
  setEditGroupTopic: (v: string) => void;

  // Actions - Edit Actor
  setEditActorRuntime: (v: SupportedRuntime) => void;
  setEditActorCommand: (v: string) => void;
  setEditActorTitle: (v: string) => void;

  // Actions - Add Actor
  setNewActorId: (v: string) => void;
  setNewActorRole: (v: "peer" | "foreman") => void;
  setNewActorRuntime: (v: SupportedRuntime) => void;
  setNewActorCommand: (v: string) => void;
  setShowAdvancedActor: (v: boolean) => void;
  setAddActorError: (v: string) => void;
  resetAddActorForm: () => void;

  // Actions - Create Group
  setCreateGroupPath: (v: string) => void;
  setCreateGroupName: (v: string) => void;
  setCreateGroupTemplateFile: (f: File | null) => void;
  setCreateWorktree: (v: boolean) => void;
  setBaseBranch: (v: string) => void;
  setBranches: (v: BranchInfo[]) => void;
  setLoadingBranches: (v: boolean) => void;
  setDirItems: (v: DirItem[]) => void;
  setDirSuggestions: (v: DirSuggestion[]) => void;
  setCurrentDir: (v: string) => void;
  setParentDir: (v: string | null) => void;
  setShowDirBrowser: (v: boolean) => void;
  resetCreateGroupForm: () => void;
}

export const useFormStore = create<FormState>((set) => ({
  // Initial state - Edit Group
  editGroupTitle: "",
  editGroupTopic: "",

  // Initial state - Edit Actor
  editActorRuntime: "codex",
  editActorCommand: "",
  editActorTitle: "",

  // Initial state - Add Actor
  newActorId: "",
  newActorRole: "peer",
  newActorRuntime: "codex",
  newActorCommand: "",
  showAdvancedActor: false,
  addActorError: "",

  // Initial state - Create Group
  createGroupPath: "",
  createGroupName: "",
  createGroupTemplateFile: null,
  createWorktree: false,
  baseBranch: "",
  branches: [],
  loadingBranches: false,
  dirItems: [],
  dirSuggestions: [],
  currentDir: "",
  parentDir: null,
  showDirBrowser: false,

  // Actions - Edit Group
  setEditGroupTitle: (v) => set({ editGroupTitle: v }),
  setEditGroupTopic: (v) => set({ editGroupTopic: v }),

  // Actions - Edit Actor
  setEditActorRuntime: (v) => set({ editActorRuntime: v }),
  setEditActorCommand: (v) => set({ editActorCommand: v }),
  setEditActorTitle: (v) => set({ editActorTitle: v }),

  // Actions - Add Actor
  setNewActorId: (v) => set({ newActorId: v }),
  setNewActorRole: (v) => set({ newActorRole: v }),
  setNewActorRuntime: (v) => set({ newActorRuntime: v }),
  setNewActorCommand: (v) => set({ newActorCommand: v }),
  setShowAdvancedActor: (v) => set({ showAdvancedActor: v }),
  setAddActorError: (v) => set({ addActorError: v }),
  resetAddActorForm: () =>
    set({
      newActorId: "",
      newActorCommand: "",
      newActorRole: "peer",
      showAdvancedActor: false,
      addActorError: "",
    }),

  // Actions - Create Group
  setCreateGroupPath: (v) => set({ createGroupPath: v }),
  setCreateGroupName: (v) => set({ createGroupName: v }),
  setCreateGroupTemplateFile: (f) => set({ createGroupTemplateFile: f }),
  setCreateWorktree: (v) => set({ createWorktree: v }),
  setBaseBranch: (v) => set({ baseBranch: v }),
  setBranches: (v) => set({ branches: v }),
  setLoadingBranches: (v) => set({ loadingBranches: v }),
  setDirItems: (v) => set({ dirItems: v }),
  setDirSuggestions: (v) => set({ dirSuggestions: v }),
  setCurrentDir: (v) => set({ currentDir: v }),
  setParentDir: (v) => set({ parentDir: v }),
  setShowDirBrowser: (v) => set({ showDirBrowser: v }),
  resetCreateGroupForm: () =>
    set({
      createGroupPath: "",
      createGroupName: "",
      createGroupTemplateFile: null,
      createWorktree: false,
      baseBranch: "",
      branches: [],
      loadingBranches: false,
      dirItems: [],
      showDirBrowser: false,
    }),
}));
