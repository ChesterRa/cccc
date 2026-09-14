import type { ConnectInstance } from "./protocol";

// Only duplicate names need an identifier in the main label. The account name
// remains unchanged, including when several instances share one host.
export function instanceName(instance: ConnectInstance, peers: ConnectInstance[]): string {
  const duplicate = peers.some(
    (peer) =>
      peer.instance_id !== instance.instance_id &&
      peer.display_name.trim().toLowerCase() === instance.display_name.trim().toLowerCase(),
  );
  return duplicate
    ? `${instance.display_name} · ${instance.instance_id.slice(-6)}`
    : instance.display_name;
}
