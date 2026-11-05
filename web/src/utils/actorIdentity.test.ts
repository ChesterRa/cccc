import { expect, it } from "vite-plus/test";
import { actorGenerationIdentity } from "./actorIdentity";

it("uses the registration generation, or the daemon's legacy identity when present", () => {
  expect(actorGenerationIdentity({ id: "a", generation: "new", created_at: "older" })).toBe("new");
  expect(actorGenerationIdentity({ id: "a", created_at: "2026-01-01T00:00:00Z" })).toBe(
    "legacy:2026-01-01T00:00:00Z",
  );
  expect(actorGenerationIdentity({ id: "a", created_at: "" })).toBe("legacy:");
  expect(actorGenerationIdentity({ id: "a" })).toBeUndefined();
});

it("does not treat a different session or registration as the same runtime owner", async () => {
  const { sameActorConfiguration } = await import("./actorIdentity");
  const native = {
    id: "a",
    generation: "g1",
    runtime: "codex",
    external_session_id: "codex:one",
    updated_at: "before",
  };
  expect(sameActorConfiguration(native, { ...native, updated_at: "after" })).toBe(true);
  expect(sameActorConfiguration(native, { ...native, external_session_id: "codex:two" })).toBe(
    false,
  );
  expect(sameActorConfiguration(native, { ...native, generation: "g2" })).toBe(false);
  expect(sameActorConfiguration(native, { ...native, runtime: "claude" })).toBe(false);
  expect(sameActorConfiguration(native, { ...native, env: { API_URL: "changed" } })).toBe(false);
  expect(sameActorConfiguration(native, { ...native, capability_autoload: ["new"] })).toBe(false);
  expect(sameActorConfiguration({ id: "a", runtime: "codex" }, { id: "a", runtime: "codex" })).toBe(
    false,
  );
});
