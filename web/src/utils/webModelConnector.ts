import type { WebModelConnector } from "../services/api";

export function findActiveWebModelConnector(
  connectors: WebModelConnector[],
  groupId: string,
  actorId: string,
): WebModelConnector | null {
  const gid = String(groupId || "").trim();
  const aid = String(actorId || "").trim();
  if (!gid || !aid) return null;
  return (
    connectors.find(
      (connector) =>
        !connector.revoked &&
        String(connector.group_id || "").trim() === gid &&
        String(connector.actor_id || "").trim() === aid,
    ) || null
  );
}

function connectorUrlWithToken(connectorUrl: string, secret: string): string {
  const url = String(connectorUrl || "").trim();
  const token = String(secret || "").trim();
  if (!url || !token) return "";
  try {
    const parsed = new URL(url);
    parsed.searchParams.set("token", token);
    return parsed.toString();
  } catch {
    const separator = url.includes("?") ? "&" : "?";
    return `${url}${separator}token=${encodeURIComponent(token)}`;
  }
}

export function webModelConnectorMcpUrl(connector?: WebModelConnector | null, secret = ""): string {
  const stored = String(connector?.connector_url_with_token || "").trim();
  if (stored) return stored;
  return connectorUrlWithToken(String(connector?.connector_url || "").trim(), secret);
}
