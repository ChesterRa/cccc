/** A comparison of the Web host and the daemon, not a compatibility verdict. */
export type DaemonVersionDifference = { webVersion: string; daemonVersion: string };

function version(value: unknown): string | null {
  if (typeof value !== "string") return null;
  const text = value.trim();
  // Cargo package versions are SemVer. Missing or malformed version data is
  // unknown, rather than evidence that another component needs to be stopped.
  if (text.length > 128 || !/^\d+\.\d+\.\d+(?:-[0-9A-Za-z.-]+)?(?:\+[0-9A-Za-z.-]+)?$/.test(text))
    return null;
  return text;
}

export function daemonVersionDifference(result: unknown): DaemonVersionDifference | null {
  if (!result || typeof result !== "object" || Array.isArray(result)) return null;
  const value = result as Record<string, unknown>;
  const daemon = value.daemon;
  if (!daemon || typeof daemon !== "object" || Array.isArray(daemon)) return null;
  const webVersion = version(value.version);
  const daemonVersion = version((daemon as Record<string, unknown>).version);
  return webVersion && daemonVersion && webVersion !== daemonVersion
    ? { webVersion, daemonVersion }
    : null;
}
