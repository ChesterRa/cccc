import { describe, expect, it } from "vite-plus/test";

import {
  buildActorSecretSaveChanges,
  emptyActorSecretChanges,
  isValidActorSecretKey,
  normalizeLoadedActorSecretKeys,
  setActorSecretClearAll,
  stageActorSecretSet,
  stageActorSecretSetMany,
  stageActorSecretUnset,
  undoActorSecretSet,
  undoActorSecretUnset,
} from "./actorSecretManagerModel";

describe("actorSecretManagerModel", () => {
  it("clears a previous load error when secret keys load successfully", () => {
    expect(
      normalizeLoadedActorSecretKeys({
        keys: ["OPENAI_API_KEY"],
        masked_values: { OPENAI_API_KEY: "sk-******1234" },
      }),
    ).toEqual({
      keys: ["OPENAI_API_KEY"],
      masks: { OPENAI_API_KEY: "sk-******1234" },
      error: "",
      loadFailed: false,
    });
  });

  it("accepts environment variable names and rejects command syntax", () => {
    expect(isValidActorSecretKey("OPENAI_API_KEY")).toBe(true);
    expect(isValidActorSecretKey("_PRIVATE_2")).toBe(true);
    expect(isValidActorSecretKey("2_INVALID")).toBe(false);
    expect(isValidActorSecretKey("unset OPENAI_API_KEY")).toBe(false);
    expect(isValidActorSecretKey("OPENAI-API-KEY")).toBe(false);
  });

  it("stages a set operation and trims the key", () => {
    const changes = stageActorSecretSet(emptyActorSecretChanges(), " NEW_KEY ", "value");

    expect(changes).toEqual({ setVars: { NEW_KEY: "value" }, unsetKeys: [], clearAll: false });
  });

  it("preserves an intentionally empty value", () => {
    const changes = stageActorSecretSet(emptyActorSecretChanges(), "EMPTY_VALUE", "");

    expect(changes.setVars).toEqual({ EMPTY_VALUE: "" });
  });

  it("staging a set cancels a pending removal for the same key", () => {
    const removed = stageActorSecretUnset(emptyActorSecretChanges(), "SHARED_KEY");
    const changes = stageActorSecretSet(removed, "SHARED_KEY", "replacement");

    expect(changes).toEqual({
      setVars: { SHARED_KEY: "replacement" },
      unsetKeys: [],
      clearAll: false,
    });
  });

  it("stages multiple set values and cancels matching pending removals", () => {
    const removed = stageActorSecretUnset(
      stageActorSecretUnset(emptyActorSecretChanges(), "FIRST_KEY"),
      "KEEP_REMOVED",
    );

    expect(stageActorSecretSetMany(removed, { FIRST_KEY: "first", SECOND_KEY: "second" })).toEqual({
      setVars: { FIRST_KEY: "first", SECOND_KEY: "second" },
      unsetKeys: ["KEEP_REMOVED"],
      clearAll: false,
    });
  });

  it("stages each removal once and discards a conflicting value", () => {
    const withValue = stageActorSecretSet(emptyActorSecretChanges(), "OLD_KEY", "replacement");
    const removed = stageActorSecretUnset(withValue, "OLD_KEY");
    const removedAgain = stageActorSecretUnset(removed, "OLD_KEY");

    expect(removedAgain).toEqual({ setVars: {}, unsetKeys: ["OLD_KEY"], clearAll: false });
  });

  it("undoes staged set and unset operations independently", () => {
    const staged = stageActorSecretUnset(
      stageActorSecretSet(emptyActorSecretChanges(), "NEW_KEY", "value"),
      "OLD_KEY",
    );

    expect(undoActorSecretSet(staged, "NEW_KEY")).toEqual({
      setVars: {},
      unsetKeys: ["OLD_KEY"],
      clearAll: false,
    });
    expect(undoActorSecretUnset(staged, "OLD_KEY")).toEqual({
      setVars: { NEW_KEY: "value" },
      unsetKeys: [],
      clearAll: false,
    });
  });

  it("keeps staged changes available when clear all is undone", () => {
    const staged = stageActorSecretSet(emptyActorSecretChanges(), "NEW_KEY", "value");
    const clearing = setActorSecretClearAll(staged, true);

    expect(clearing).toEqual({ setVars: { NEW_KEY: "value" }, unsetKeys: [], clearAll: true });
    expect(setActorSecretClearAll(clearing, false)).toEqual(staged);
  });

  it("builds a normal save payload from staged changes", () => {
    const changes = stageActorSecretUnset(
      stageActorSecretSet(emptyActorSecretChanges(), "NEW_KEY", "value"),
      "OLD_KEY",
    );

    expect(buildActorSecretSaveChanges(changes)).toEqual({
      setVars: { NEW_KEY: "value" },
      unsetKeys: ["OLD_KEY"],
      clear: false,
    });
  });

  it("emits only clear when clear all is staged", () => {
    const changes = setActorSecretClearAll(
      stageActorSecretUnset(
        stageActorSecretSet(emptyActorSecretChanges(), "NEW_KEY", "value"),
        "OLD_KEY",
      ),
      true,
    );

    expect(buildActorSecretSaveChanges(changes)).toEqual({
      setVars: {},
      unsetKeys: [],
      clear: true,
    });
  });
});
