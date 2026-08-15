export function getEffectiveComposerDestGroupId(
  destGroupId: string,
  activeGroupId: string,
  selectedGroupId: string,
): string {
  const selected = String(selectedGroupId || "").trim();
  const active = String(activeGroupId || "").trim();
  const dest = String(destGroupId || "").trim();

  if (!selected) return dest;
  // During the first frame after a group switch, composer state may still belong
  // to the previous group; avoid carrying that old destination into the new group.
  if (active !== selected) return selected;
  return dest || selected;
}

export function isComposerGroupSettled(activeGroupId: string, selectedGroupId: string): boolean {
  return String(activeGroupId || "").trim() === String(selectedGroupId || "").trim();
}

export function getComposerDestGroupDisplayValue(
  destGroupId: string,
  selectedGroupId: string,
  composerGroupSettled: boolean,
): string {
  const selected = String(selectedGroupId || "").trim();
  if (!composerGroupSettled) return selected;
  return String(destGroupId || "").trim() || selected;
}
