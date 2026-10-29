import { createContext } from "react";
import type { LedgerEventStatusPayload } from "../../types";

/** A previous status may be displayed while checking, but cannot authorize a send. */
export type LivePendingStatus = { status: LedgerEventStatusPayload | null; checking: boolean };
export const LivePendingStatusContext = createContext<Record<string, LivePendingStatus> | null>(
  null,
);
