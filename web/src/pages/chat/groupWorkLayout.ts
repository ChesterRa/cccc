export function terminalPageLayout(actorCount: number, preferredPage: number, width: number) {
  const pageSize = width > 0 && width < 720 ? 1 : 4;
  const pageCount = Math.max(1, Math.ceil(actorCount / pageSize));
  const page = Math.min(Math.max(0, preferredPage), pageCount - 1);
  return { page, pageCount, pageSize, start: page * pageSize };
}
