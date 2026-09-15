import type { GroupMeta } from "../../types";

export type ConnectInstance = {
  instance_id: string;
  device_id: string;
  public_key: string;
  public_origin: string | null;
  client_version: string;
  display_name: string;
};

export type ConnectSnapshot = {
  account_origin: string;
  device_id: string;
  instance_id: string;
  checked_at: string;
  directory: { expires_at: string; instances: ConnectInstance[] } | null;
  error_code: string | null;
  error_message: string | null;
};

export type FrameProof = {
  account_origin: string;
  source_instance_id: string;
  source_device_id: string;
  target_instance_id: string;
  target_device_id: string;
  parent_origin: string;
  frame_id: string;
  nonce: string;
  issued_at: string;
  expires_at: string;
  signature: string;
};

export type OpenFrame = { origin: string; url: string; proof: FrameProof };
export type GroupConnectionSummary = { counts: Record<string, number | null>; expires_at: string };
export type GroupConnectionCount = { count: number | null; expires_at: string };
export type ConnectStatusResponse = {
  connect: ConnectSnapshot | null;
  account_label?: string | null;
  group_connections?: GroupConnectionSummary | null;
};
export type RemoteGroup = Pick<GroupMeta, "group_id" | "title" | "running"> & {
  connection?: GroupConnectionCount;
};
export function groupConnectionCount(
  summary: GroupConnectionSummary | null | undefined,
  groupId: string,
): GroupConnectionCount | undefined {
  return summary
    ? {
        count: summary.counts[groupId] === undefined ? 0 : summary.counts[groupId],
        expires_at: summary.expires_at,
      }
    : undefined;
}
export const CONNECT_CHANNEL = "cccc.connect.frame.v1";

export function isConnectFramePath(path: string): boolean {
  return path === "/ui/connect" || path === "/ui/connect/";
}

// The server verifies the signature before serving this document. This parser
// only extracts the already admitted frame's routing and lifetime information.
export function readFrameProof(location: Pick<Location, "pathname" | "search">): FrameProof | null {
  if (!isConnectFramePath(location.pathname)) return null;
  try {
    const raw = new URLSearchParams(location.search).get("proof") || "";
    if (!raw || raw.length > 4096) return null;
    const value = JSON.parse(
      new TextDecoder().decode(
        Uint8Array.from(atob(raw.replace(/-/g, "+").replace(/_/g, "/")), (c) => c.charCodeAt(0)),
      ),
    ) as FrameProof;
    if (
      !value ||
      typeof value !== "object" ||
      ![
        value.frame_id,
        value.target_instance_id,
        value.target_device_id,
        value.parent_origin,
        value.expires_at,
        value.signature,
      ].every((v) => typeof v === "string" && v.length > 0) ||
      !Number.isFinite(Date.parse(value.expires_at)) ||
      new URL(value.parent_origin).origin !== value.parent_origin
    )
      return null;
    return value;
  } catch {
    return null;
  }
}

export function acceptsFrameMessage(
  event: MessageEvent,
  peer: Window | null,
  origin: string,
  frameId: string,
): boolean {
  return (
    peer !== null &&
    event.source === peer &&
    event.origin === origin &&
    event.data !== null &&
    typeof event.data === "object" &&
    event.data.channel === CONNECT_CHANNEL &&
    event.data.frame_id === frameId
  );
}

// The initial frame proof is single-use. After logout or listener replacement,
// return control to the entry instead of navigating with a consumed proof.
export function endConnectFrame(): boolean {
  const proof = readFrameProof(window.location);
  if (!proof || window.parent === window) return false;
  window.parent.postMessage(
    { channel: CONNECT_CHANNEL, frame_id: proof.frame_id, type: "expired" },
    proof.parent_origin,
  );
  return true;
}

export function remoteGroups(value: unknown): RemoteGroup[] | null {
  if (!Array.isArray(value) || value.length > 4096) return null;
  if (
    !value.every(
      (g) =>
        g &&
        typeof g === "object" &&
        typeof g.group_id === "string" &&
        g.group_id.length <= 256 &&
        typeof g.title === "string" &&
        g.title.length <= 4096 &&
        typeof g.running === "boolean",
    )
  )
    return null;
  return value.map((g) => ({
    group_id: g.group_id,
    title: g.title,
    running: g.running,
    ...((g.connection?.count === null ||
      (Number.isSafeInteger(g.connection?.count) &&
        g.connection.count >= 0 &&
        g.connection.count <= 128)) &&
    typeof g.connection.expires_at === "string" &&
    Number.isFinite(Date.parse(g.connection.expires_at))
      ? { connection: { count: g.connection.count, expires_at: g.connection.expires_at } }
      : {}),
  }));
}

export function frameResourceUrl(url: string, location: Location = window.location): string {
  const proof = readFrameProof(location);
  if (!proof) return url;
  try {
    const target = new URL(url, location.href);
    // A WebSocket belongs to the corresponding HTTP origin. Compare that
    // origin without changing the actual resource's scheme or port.
    const httpTarget = new URL(target);
    if (httpTarget.protocol === "wss:") httpTarget.protocol = "https:";
    else if (httpTarget.protocol === "ws:") httpTarget.protocol = "http:";
    if (
      httpTarget.origin !== location.origin ||
      !(
        target.pathname.startsWith("/api/v1/groups/") || target.pathname === "/api/v1/events/stream"
      )
    )
      return url;
    target.searchParams.set("connect_frame", proof.frame_id);
    return /^[a-z][a-z\d+.-]*:/i.test(url)
      ? target.toString()
      : `${target.pathname}${target.search}${target.hash}`;
  } catch {
    return url;
  }
}
