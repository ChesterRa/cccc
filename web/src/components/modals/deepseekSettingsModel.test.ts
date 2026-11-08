import { describe, expect, it } from "vite-plus/test";
import { emptyActorSecretChanges } from "./actorSecretManagerModel";
import {
  DEEPSEEK_MODEL,
  deepSeekSettingsError,
  normalizeDeepSeekSettings,
} from "./deepseekSettingsModel";

describe("DeepSeek connection settings", () => {
  it("suggests the model name DeepSeek documents for its API", () => {
    expect(DEEPSEEK_MODEL).toBe("deepseek-flash");
  });

  it("stores a base URL, a trimmed model and a trimmed key", () => {
    const changes = {
      ...emptyActorSecretChanges(),
      setVars: {
        DEEPSEEK_BASE_URL: " https://gateway.example/v1/chat/completions/ ",
        DEEPSEEK_MODEL: " channel/custom-model ",
        DEEPSEEK_API_KEY: " sk-fixture\n",
        OTHER_FIXTURE: " kept as typed ",
      },
      unsetKeys: ["OLD_FIXTURE"],
    };
    expect(normalizeDeepSeekSettings(changes)).toEqual({
      ...changes,
      setVars: {
        DEEPSEEK_BASE_URL: "https://gateway.example/v1",
        DEEPSEEK_MODEL: "channel/custom-model",
        DEEPSEEK_API_KEY: "sk-fixture",
        OTHER_FIXTURE: " kept as typed ",
      },
    });
  });

  it("keeps a saved value when a field holds only spaces", () => {
    const changes = {
      ...emptyActorSecretChanges(),
      setVars: { DEEPSEEK_API_KEY: "  \n", DEEPSEEK_MODEL: " ", DEEPSEEK_BASE_URL: "/" },
    };
    expect(normalizeDeepSeekSettings(changes).setVars).toEqual({});
  });

  it("rejects addresses with credentials, queries, fragments or other schemes", () => {
    for (const address of [
      "https://user:secret@gateway.example/v1",
      "https://gateway.example/v1?key=fixture",
      "https://gateway.example/v1#fragment",
      "ftp://gateway.example/v1",
      "gateway.example/v1",
    ]) {
      expect(deepSeekSettingsError({ DEEPSEEK_BASE_URL: address }), address).toBe("hostInvalid");
    }
    expect(deepSeekSettingsError({ DEEPSEEK_BASE_URL: "http://127.0.0.1:8080/v1" })).toBe("");
    expect(deepSeekSettingsError({ DEEPSEEK_BASE_URL: "https://api.deepseek.com" })).toBe("");
  });

  it("rejects model IDs with whitespace or over 256 characters", () => {
    expect(deepSeekSettingsError({ DEEPSEEK_MODEL: "deepseek flash" })).toBe("modelInvalid");
    expect(deepSeekSettingsError({ DEEPSEEK_MODEL: "m".repeat(257) })).toBe("modelInvalid");
    expect(deepSeekSettingsError({ DEEPSEEK_MODEL: "deepseek-v4-pro" })).toBe("");
    expect(deepSeekSettingsError({})).toBe("");
  });
});
