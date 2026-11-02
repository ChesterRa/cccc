import { expect, it } from "vite-plus/test";

import { CHET_NAME, chetGeneration, isChetMessage } from "./chetIdentity";
import { getRecipientDisplayName } from "../hooks/useActorDisplayName";
import type { LedgerEvent } from "../types";

const event = (marker: unknown, by = "svc:chet"): LedgerEvent => ({
  id: "e1",
  kind: "chat.message",
  by,
  data: { text: "Please check the build.", to: ["user"], _cccc_chet_group_generation: marker },
});

it("proves Chet only by the Group generation its message was written in", () => {
  expect(isChetMessage(event("generation-1"), "generation-1")).toBe(true);
  // A copied or re-created Group has a new generation; its old messages prove nothing.
  expect(isChetMessage(event("generation-0"), "generation-1")).toBe(false);
  expect(isChetMessage(event(undefined), "generation-1")).toBe(false);
  // Until the Group is loaded, nothing is proven.
  expect(isChetMessage(event("generation-1"), "")).toBe(false);
  expect(isChetMessage(event("generation-1", "worker"), "generation-1")).toBe(false);
});

it("reads the generation a Group without a stored one gives Chet's messages", () => {
  expect(chetGeneration({ generation: "generation-1", created_at: "2026-01-01T00:00:00Z" })).toBe(
    "generation-1",
  );
  expect(chetGeneration({ generation: "", created_at: "2026-01-01T00:00:00Z" })).toBe(
    "legacy:2026-01-01T00:00:00Z",
  );
  expect(chetGeneration({})).toBe("");
  expect(chetGeneration(null)).toBe("");
});

it("names Chet as a recipient", () => {
  expect(getRecipientDisplayName("svc:chet", new Map())).toBe(CHET_NAME);
  expect(getRecipientDisplayName("@chet", new Map())).toBe("@chet");
});
