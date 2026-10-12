import { isLocalWebHostname } from "../../../utils/webAddress";

export type ConnectorAddress =
  | { kind: "https" }
  | { kind: "missing" }
  | { kind: "invalid"; origin: string };

/**
 * Check the configured address before a connector exists. HTTPS on a known
 * local host cannot be used by an external service. Other HTTPS addresses are
 * only configuration candidates; this does not test DNS, TLS or connectivity.
 * Notices show the origin, never the private connector path.
 */
export function connectorAddress(address: string): ConnectorAddress {
  let url: URL;
  try {
    url = new URL(address);
  } catch {
    return { kind: "missing" };
  }
  if (url.protocol !== "http:" && url.protocol !== "https:") return { kind: "missing" };
  if (url.protocol === "https:" && !isLocalWebHostname(url.hostname)) {
    return { kind: "https" };
  }
  return { kind: "invalid", origin: url.origin };
}
