export function getComposerCanSend({
  composerText,
  composerFilesCount,
  recipientResolutionBusy: _recipientResolutionBusy = false,
  messageMode = "send",
  toTokens = [],
  hasRemoteGroupSelection = false,
}: {
  composerText: string;
  composerFilesCount: number;
  recipientResolutionBusy?: boolean;
  messageMode?: "send" | "request_reply" | "mail";
  toTokens?: string[];
  hasRemoteGroupSelection?: boolean;
}): boolean {
  const hasContent = String(composerText || "").trim().length > 0 || composerFilesCount > 0;
  return (
    hasContent &&
    (messageMode !== "request_reply" ||
      hasConcreteReplyRecipients(toTokens, hasRemoteGroupSelection))
  );
}

export function hasConcreteReplyRecipients(
  toTokens: string[],
  hasRemoteGroupSelection = false,
): boolean {
  if (hasRemoteGroupSelection) return false;
  if (toTokens.length === 0) return true;
  const nonConcrete = new Set(["@all", "@peers", "@user", "user"]);
  return toTokens.every((token) => {
    const recipient = String(token || "").trim();
    return recipient.length > 0 && !nonConcrete.has(recipient);
  });
}
