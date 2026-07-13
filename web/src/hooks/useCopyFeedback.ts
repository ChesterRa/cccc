import { useCallback } from "react";
import { useUIStore } from "../stores";
import { copyTextToClipboard } from "../utils/copy";

type CopyFeedbackOptions = { successMessage?: string; errorMessage?: string };

export function useCopyFeedback() {
  const showNotice = useUIStore((state) => state.showNotice);
  const showError = useUIStore((state) => state.showError);

  return useCallback(
    async (value: string, options: CopyFeedbackOptions = {}): Promise<boolean> => {
      const ok = await copyTextToClipboard(value);
      if (ok) {
        if (options.successMessage) {
          showNotice({ message: options.successMessage });
        }
        return true;
      }
      if (options.errorMessage) {
        showError(options.errorMessage);
      }
      return false;
    },
    [showError, showNotice],
  );
}
