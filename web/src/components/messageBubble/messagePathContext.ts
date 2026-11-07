import { createContext } from "react";

/** The workspace a message's file paths open in. Absent where nothing can open them. */
export type MessagePathTarget = {
  scopeKey: string;
  rootPath: string;
  open: (path: string) => void;
};

export const MessagePathContext = createContext<MessagePathTarget | null>(null);

/** A path from another Group, instance or scope means a different file here. */
export function messagePathTarget(
  target: MessagePathTarget | null,
  message: { scopeKey: string; foreign: boolean },
) {
  if (!target || message.foreign) return null;
  return !message.scopeKey || message.scopeKey === target.scopeKey ? target : null;
}
