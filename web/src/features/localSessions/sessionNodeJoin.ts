const requestParam = "session_node_join";

export function linkedSessionNodeJoin(): string | null {
  if (typeof window === "undefined") return null;
  return new URLSearchParams(window.location.search).get(requestParam);
}

export function clearSessionNodeJoinLink() {
  const url = new URL(window.location.href);
  url.searchParams.delete(requestParam);
  window.history.replaceState(window.history.state, "", url);
}

/** Only copy a remote HTTPS origin that is also safe as a terminal argument. */
export function shareableSessionNodeOrigin(value: string | null | undefined): string | null {
  if (!value) return null;
  try {
    const url = new URL(value);
    const host = url.hostname.toLowerCase().replace(/\.$/, "");
    if (
      url.protocol !== "https:" ||
      url.port === "0" ||
      url.username ||
      url.password ||
      url.pathname !== "/" ||
      url.search ||
      url.hash ||
      !/^(?:[a-z0-9.-]+|\[[0-9a-f:]+\])$/.test(host) ||
      host === "localhost" ||
      host === "host.docker.internal" ||
      host.endsWith(".localhost") ||
      /^(?:0|127)\.\d+\.\d+\.\d+$/.test(host) ||
      host === "[::]" ||
      host === "[::1]" ||
      /^\[::ffff:(?:7f[0-9a-f]{2}|[0-9a-f]{1,2}):/.test(host)
    )
      return null;
    return url.origin;
  } catch {
    return null;
  }
}

export function sessionNodeJoinCommand(
  advertised: string | null | undefined,
  browserOrigin: string,
) {
  const origin =
    shareableSessionNodeOrigin(advertised) ?? shareableSessionNodeOrigin(browserOrigin);
  if (!origin) return null;
  // Bracketed IPv6 origins need quoting in shells such as zsh.
  return `cccc sessions node join ${origin.includes("[") ? `"${origin}"` : origin}`;
}
