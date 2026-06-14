import type { WebModelBrowserSession } from "../services/api";

export type TargetDraftMode = "existing" | "new";

export type TargetDraft = {
  mode: TargetDraftMode;
  url: string;
};

export function isChatGptConversationUrl(url?: string): boolean {
  const raw = String(url || "").trim();
  if (!raw) return false;
  try {
    const parsed = new URL(raw);
    if (parsed.protocol !== "https:") return false;
    if (parsed.hostname !== "chatgpt.com" && !parsed.hostname.endsWith(".chatgpt.com")) return false;
    const parts = parsed.pathname.split("/").filter(Boolean);
    return parts.some((part, index) => part === "c" && Boolean(parts[index + 1]));
  } catch {
    return false;
  }
}

export function liveBrowserConversationUrlFromSession(session?: WebModelBrowserSession | null): string {
  const liveUrl = String(session?.tab_url || "").trim();
  return isChatGptConversationUrl(liveUrl) ? liveUrl : "";
}

type TargetLike = {
  state?: string;
  url?: string;
};

function firstChatGptConversationUrl(...values: Array<string | undefined>): string {
  for (const value of values) {
    const url = String(value || "").trim();
    if (isChatGptConversationUrl(url)) return url;
  }
  return "";
}

function savedTargetFromSession(session?: WebModelBrowserSession | null): TargetLike | null {
  const candidates: TargetLike[] = [
    session?.delivery_target || {},
    session?.health_snapshot?.delivery_target || {},
    session?.health_snapshot?.target || {},
  ];
  return candidates.find((target) => {
    const state = String(target.state || "").trim();
    return Boolean(target.url || (state && state !== "none"));
  }) || candidates[0] || null;
}

export function savedTargetDraftFromSession(session?: WebModelBrowserSession | null): TargetDraft {
  const target = savedTargetFromSession(session);
  const targetState = String(target?.state || "").trim();
  const conversationUrl = firstChatGptConversationUrl(
    target?.url,
    session?.health_snapshot?.target?.url,
    session?.conversation_url,
  );
  if (targetState === "bound_existing_chat" || isChatGptConversationUrl(conversationUrl)) {
    return { mode: "existing", url: conversationUrl };
  }
  if (targetState === "new_chat_armed" || targetState === "new_chat_submitted" || session?.pending_new_chat_bind) {
    return { mode: "new", url: "" };
  }
  return { mode: "existing", url: "" };
}

export function defaultTargetDraftFromSession(
  session?: WebModelBrowserSession | null,
  currentBrowserConversationUrl = "",
): TargetDraft {
  const saved = savedTargetDraftFromSession(session);
  if (saved.mode === "existing" && saved.url) return saved;
  if (saved.mode === "new") return saved;
  const current = String(currentBrowserConversationUrl || "").trim();
  if (isChatGptConversationUrl(current)) {
    return { mode: "existing", url: current };
  }
  return { mode: "existing", url: "" };
}

export function targetDraftMatchesSaved({
  mode,
  url,
  saved,
}: {
  mode: TargetDraftMode;
  url: string;
  saved: TargetDraft;
}): boolean {
  const normalizedUrl = String(url || "").trim();
  const savedUrl = String(saved.url || "").trim();
  if (mode === "new") return saved.mode === "new";
  if (mode === "existing") return saved.mode === "existing" && normalizedUrl === savedUrl;
  return false;
}
