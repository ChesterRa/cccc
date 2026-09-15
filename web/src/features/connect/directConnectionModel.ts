export type DirectEndpoint = { instance_id: string; name: string; group_id: string; title: string };
export type DirectRelation = {
  id: string;
  local: DirectEndpoint;
  remote: DirectEndpoint | null;
  state: "invited" | "pending" | "active" | "expired" | "revoked";
  initiated: boolean;
  current: boolean;
  expired: boolean;
  expires_at: string;
  online: boolean;
  error: string | null;
};
export type DirectListener = { bind: string; address: string };
export type DirectAddress = DirectListener & { interface: string };
export type DirectStatus = {
  display_name: string;
  listener: DirectListener | null;
  addresses: DirectAddress[];
  runtime: { listener: boolean; error: string | null } | null;
  relations: DirectRelation[];
};

export function sameDirectListener(a: DirectListener | null, b: DirectListener | null): boolean {
  return a?.address === b?.address && a?.bind === b?.bind;
}
export type DirectInvitationPreview = {
  id: string;
  host: DirectEndpoint;
  address: string;
  expires_at: string;
};

/** Display untrusted invitation metadata only. The daemon validates all authority. */
export function readDirectInvitation(text: string): DirectInvitationPreview | null {
  const value = text.trim();
  if (value.length > 8192 || !value.startsWith("cccc-direct:")) return null;
  try {
    const raw = value.slice("cccc-direct:".length).replace(/-/g, "+").replace(/_/g, "/");
    const data = JSON.parse(
      new TextDecoder("utf-8", { fatal: true }).decode(
        Uint8Array.from(atob(raw), (c) => c.charCodeAt(0)),
      ),
    );
    if (
      data?.v !== 1 ||
      typeof data.secret !== "string" ||
      data.secret.length !== 64 ||
      ![data.id, data.address, data.expires_at, data.host?.instance_id, data.host?.group_id].every(
        (s) => typeof s === "string" && s.length > 0,
      ) ||
      typeof data.host?.name !== "string" ||
      typeof data.host?.title !== "string" ||
      !Number.isFinite(Date.parse(data.expires_at))
    )
      return null;
    return {
      id: data.id,
      host: {
        ...data.host,
        name: data.host.name || data.host.instance_id,
        title: data.host.title || data.host.group_id,
      },
      address: data.address,
      expires_at: data.expires_at,
    };
  } catch {
    return null;
  }
}

export function directRelationState(relation: DirectRelation): string {
  if (!relation.current) return "replaced";
  if (relation.state === "revoked") return "revoked";
  if (relation.state === "expired") return "expired";
  if (relation.state !== "active" && relation.expired)
    return relation.initiated ? "checkingApproval" : "expired";
  if (relation.online) return "online";
  if (relation.state === "pending") return relation.initiated ? "pending" : "needsApproval";
  return relation.state;
}

export function directRelationClosed(relation: DirectRelation): boolean {
  return (
    relation.state === "revoked" ||
    relation.state === "expired" ||
    (relation.state !== "active" && relation.expired && !relation.initiated)
  );
}
