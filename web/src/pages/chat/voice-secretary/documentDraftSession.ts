/** A draft belongs to one load, even if the user later returns to the same path. */
export type VoiceDocumentDraftOwner = { path: string; session: number };
export type VoiceDocumentDraftSnapshot = VoiceDocumentDraftOwner & {
  content: string;
  dirty: boolean;
};

export function sameVoiceDocumentGeneration(
  left: VoiceDocumentDraftOwner,
  right: VoiceDocumentDraftOwner,
): boolean {
  return left.session === right.session;
}

export function sameVoiceDocumentSession(
  left: VoiceDocumentDraftOwner,
  right: VoiceDocumentDraftOwner,
): boolean {
  return sameVoiceDocumentGeneration(left, right) && left.path === right.path;
}
