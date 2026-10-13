export async function copyTextToClipboard(value: string): Promise<boolean> {
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
  const textarea = document.createElement("textarea");
  try {
    textarea.value = text;
    textarea.setAttribute("readonly", "true");
    textarea.style.position = "fixed";
    textarea.style.left = "-9999px";
    textarea.style.top = "0";
    textarea.style.opacity = "0";
    document.body.appendChild(textarea);
    textarea.focus();
    textarea.select();
    return !!document.execCommand("copy");
  } catch {
    return false;
  } finally {
    // Copied text may be a credential: never leave it in the page, even when
    // the legacy copy throws.
    try {
      document.body.removeChild(textarea);
    } catch {
      // It was never attached.
    }
  }
}
