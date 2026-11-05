import { isWebModelRuntime, type Actor } from "../../types";

export type RuntimeCategory = "managed" | "native" | "web";

export function actorRuntimeCategory(actor?: Actor): RuntimeCategory {
  if (actor?.external_session_id) return "native";
  return isWebModelRuntime(actor?.runtime) ? "web" : "managed";
}
