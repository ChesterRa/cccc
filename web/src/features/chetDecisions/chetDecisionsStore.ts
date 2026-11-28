import { create } from "zustand";

/** Whether Chet's page shows the cross-Group decision queue in place of its conversation. */
export const useChetDecisionsStore = create<{ open: boolean; setOpen: (open: boolean) => void }>(
  (set) => ({ open: false, setOpen: (open) => set({ open }) }),
);
