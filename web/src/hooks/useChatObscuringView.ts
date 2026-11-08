import { useLayoutEffect } from "react";
import { useUIStore } from "../stores/useUIStore";
import { useMediaQuery } from "./useMediaQuery";

/** Register large views that cover chat, not menus, small dialogs, or split panels. */
export function useChatObscuringView(open: boolean): void {
  useLayoutEffect(() => {
    if (!open) return;
    return useUIStore.getState().registerChatObscuringView();
  }, [open]);
}

/** Some dialogs only cover chat below the `sm` breakpoint, where they become full-screen. */
export function useChatObscuringViewOnPhone(open: boolean): void {
  const fullScreen = useMediaQuery("(width < 40rem)");
  useChatObscuringView(open && fullScreen);
}
