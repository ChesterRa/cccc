export function getComposerCanSend({
  composerText,
  composerFilesCount,
  recipientResolutionBusy: _recipientResolutionBusy = false,
  messageMode = "send",
  toTokens = [],
}: {
  composerText: string;
  composerFilesCount: number;
  recipientResolutionBusy?: boolean;
  messageMode?: "send" | "request_reply" | "mail";
  toTokens?: string[];
}): boolean {
  const hasContent = String(composerText || "").trim().length > 0 || composerFilesCount > 0;
  return hasContent && (messageMode !== "request_reply" || hasConcreteReplyRecipients(toTokens));
}

export function hasConcreteReplyRecipients(toTokens: string[]): boolean {
  if (toTokens.length === 0) return true;
  const nonConcrete = new Set(["@all", "@peers", "@user", "user"]);
  return toTokens.every((token) => {
    const recipient = String(token || "").trim();
    return recipient.length > 0 && !nonConcrete.has(recipient);
  });
}
