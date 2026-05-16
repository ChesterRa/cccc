export interface ActorActionInFlightRef {
  current: Set<string>;
}

export function beginActorAction(inFlight: ActorActionInFlightRef, key: string): boolean {
  if (inFlight.current.has(key)) return false;
  inFlight.current.add(key);
  return true;
}

export function endActorAction(inFlight: ActorActionInFlightRef, key: string): void {
  inFlight.current.delete(key);
}
