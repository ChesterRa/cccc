// Form state store for modals.
import { create } from "zustand";
import type { DirItem, DirSuggestion, SupportedRuntime } from "../types";

interface FormState {
  // Edit Group
  editGroupTitle: string;
  editGroupTopic: string;

  // Edit Actor
  editActorRuntime: SupportedRuntime;
  editActorRunner: "pty" | "headless";
  editActorCommand: string;
  editActorTitle: string;
  editActorRoleNotes: string;
  editActorCapabilityAutoloadText: string;

  // Add Actor
  newActorId: string;
  newActorRole: "peer" | "foreman";
  newActorRuntime: SupportedRuntime;
  newActorRunner: "pty" | "headless";
  newActorCommand: string;
  newActorUseDefaultCommand: boolean;
  newActorSecretsSetText: string;
  newActorCapabilityAutoloadText: string;
  newActorRoleNotes: string;
  newActorUseProfile: boolean;
  newActorProfileId: string;
  addActorError: string;

  // Create Group
  createGroupPath: string;
  createGroupName: string;
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
  setEditActorRunner: (v: "pty" | "headless") => void;
  setEditActorCommand: (v: string) => void;
  setEditActorTitle: (v: string) => void;
  setEditActorRoleNotes: (v: string) => void;
  setEditActorCapabilityAutoloadText: (v: string) => void;

  // Actions - Add Actor
  setNewActorId: (v: string) => void;
  setNewActorRole: (v: "peer" | "foreman") => void;
  setNewActorRuntime: (v: SupportedRuntime) => void;
  setNewActorRunner: (v: "pty" | "headless") => void;
  setNewActorCommand: (v: string) => void;
  setNewActorUseDefaultCommand: (v: boolean) => void;
  setNewActorSecretsSetText: (v: string) => void;
  setNewActorCapabilityAutoloadText: (v: string) => void;
  setNewActorRoleNotes: (v: string) => void;
  setNewActorUseProfile: (v: boolean) => void;
  setNewActorProfileId: (v: string) => void;
  setAddActorError: (v: string) => void;
  resetAddActorForm: () => void;

  // Actions - Create Group
  setCreateGroupPath: (v: string) => void;
  setCreateGroupName: (v: string) => void;
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
  editActorRunner: "pty",
  editActorCommand: "",
  editActorTitle: "",
  editActorRoleNotes: "",
  editActorCapabilityAutoloadText: "",

  // Initial state - Add Actor
  newActorId: "",
  newActorRole: "peer",
  newActorRuntime: "codex",
  newActorRunner: "pty",
  newActorCommand: "",
  newActorUseDefaultCommand: true,
  newActorSecretsSetText: "",
  newActorCapabilityAutoloadText: "",
  newActorRoleNotes: "",
  newActorUseProfile: false,
  newActorProfileId: "",
  addActorError: "",

  // Initial state - Create Group
  createGroupPath: "",
  createGroupName: "",
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
  setEditActorRunner: (v) => set({ editActorRunner: v }),
  setEditActorCommand: (v) => set({ editActorCommand: v }),
  setEditActorTitle: (v) => set({ editActorTitle: v }),
  setEditActorRoleNotes: (v) => set({ editActorRoleNotes: v }),
  setEditActorCapabilityAutoloadText: (v) => set({ editActorCapabilityAutoloadText: v }),

  // Actions - Add Actor
  setNewActorId: (v) => set({ newActorId: v }),
  setNewActorRole: (v) => set({ newActorRole: v }),
  setNewActorRuntime: (v) => set({ newActorRuntime: v }),
  setNewActorRunner: (v) => set({ newActorRunner: v }),
  setNewActorCommand: (v) => set({ newActorCommand: v }),
  setNewActorUseDefaultCommand: (v) => set({ newActorUseDefaultCommand: v }),
  setNewActorSecretsSetText: (v) => set({ newActorSecretsSetText: v }),
  setNewActorCapabilityAutoloadText: (v) => set({ newActorCapabilityAutoloadText: v }),
  setNewActorRoleNotes: (v) => set({ newActorRoleNotes: v }),
  setNewActorUseProfile: (v) => set({ newActorUseProfile: v }),
  setNewActorProfileId: (v) => set({ newActorProfileId: v }),
  setAddActorError: (v) => set({ addActorError: v }),
  resetAddActorForm: () =>
    set({
      newActorId: "",
      newActorCommand: "",
      newActorRuntime: "codex",
      newActorRunner: "pty",
      newActorUseDefaultCommand: true,
      newActorSecretsSetText: "",
      newActorCapabilityAutoloadText: "",
      newActorRoleNotes: "",
      newActorUseProfile: false,
      newActorProfileId: "",
      newActorRole: "peer",
      addActorError: "",
    }),

  // Actions - Create Group
  setCreateGroupPath: (v) => set({ createGroupPath: v }),
  setCreateGroupName: (v) => set({ createGroupName: v }),
  setDirItems: (v) => set({ dirItems: v }),
  setDirSuggestions: (v) => set({ dirSuggestions: v }),
  setCurrentDir: (v) => set({ currentDir: v }),
  setParentDir: (v) => set({ parentDir: v }),
  setShowDirBrowser: (v) => set({ showDirBrowser: v }),
  resetCreateGroupForm: () =>
    set({
      createGroupPath: "",
      createGroupName: "",
      dirItems: [],
      showDirBrowser: false,
    }),
}));
