import { createContext } from "react";

/** Identify the focused editor so an older virtual row cannot clear a newer one. */
export const PendingReplyEditingContext = createContext<
  ((editorId: string, focused: boolean) => void) | undefined
>(undefined);
