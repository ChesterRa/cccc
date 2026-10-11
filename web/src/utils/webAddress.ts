/** Known local addresses; this is an address hint, not a reachability check. */
export function isLocalWebHostname(hostname: string): boolean {
  const host = hostname.trim().toLowerCase();
  if (
    host === "localhost" ||
    host === "::1" ||
    host === "[::1]" ||
    host === "host.docker.internal" ||
    host.endsWith(".local")
  ) {
    return true;
  }
  const parts = host.split(".").map((part) => Number(part));
  if (
    parts.length !== 4 ||
    parts.some((part) => !Number.isInteger(part) || part < 0 || part > 255)
  ) {
    return false;
  }
  return (
    parts[0] === 10 ||
    parts[0] === 127 ||
    parts[0] === 0 ||
    (parts[0] === 192 && parts[1] === 168) ||
    (parts[0] === 172 && parts[1] >= 16 && parts[1] <= 31) ||
    (parts[0] === 169 && parts[1] === 254)
  );
}
