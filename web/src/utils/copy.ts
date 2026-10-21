export async function copyTextToClipboard(
  value: string,
  fallbackTarget?: HTMLTextAreaElement | null,
): Promise<boolean> {
  const text = String(value || "");
  if (!text.trim()) return false;

  try {
    if (typeof navigator !== "undefined" && navigator.clipboard?.writeText) {
      await navigator.clipboard.writeText(text);
      return true;
    }
  } catch {
    // Fall through to the DOM copy fallback below.
  }

  if (typeof document === "undefined" || !document.body) return false;
  // Reuse the visible field when available: modal focus traps can prevent an
  // off-screen field attached to body from owning the clipboard selection.
  const existing = fallbackTarget?.isConnected && fallbackTarget.value === text;
  const textarea = existing ? fallbackTarget : document.createElement("textarea");
  try {
    if (!existing) {
      textarea.value = text;
      textarea.setAttribute("readonly", "true");
      textarea.style.position = "fixed";
      textarea.style.left = "-9999px";
      textarea.style.top = "0";
      textarea.style.opacity = "0";
      const modal = document.activeElement?.closest('[role="dialog"], dialog');
      (modal || document.body).appendChild(textarea);
    }
    textarea.focus();
    textarea.select();
    textarea.setSelectionRange(0, text.length);
    if (document.activeElement !== textarea) return false;
    return !!document.execCommand("copy");
  } catch {
    return false;
  } finally {
    // Copied text may be a credential: never leave it in the page, even when
    // the legacy copy throws.
    try {
      if (!existing) textarea.remove();
    } catch {
      // It was never attached.
    }
  }
}
