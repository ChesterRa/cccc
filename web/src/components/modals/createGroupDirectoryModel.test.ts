import { describe, expect, it } from "vite-plus/test";
import {
  directoryNameFromPath,
  driveSuggestions,
  suggestionLabelKey,
} from "./createGroupDirectoryModel";

describe("create group directory model", () => {
  it("extracts directory names from Windows and POSIX paths", () => {
    expect(directoryNameFromPath("C:\\Users\\demo\\project")).toBe("project");
    expect(directoryNameFromPath("D:\\")).toBe("D:");
    expect(directoryNameFromPath("/Users/demo/project/")).toBe("project");
  });

  it("keeps Windows drive locations available in the open browser", () => {
    expect(
      driveSuggestions([
        { name: "Home", path: "C:\\Users\\demo", icon: "home" },
        { name: "Drive C:", path: "C:\\", icon: "drive" },
      ]),
    ).toEqual([{ name: "Drive C:", path: "C:\\", icon: "drive" }]);
  });

  it("translates only the folders the server names in English", () => {
    expect(suggestionLabelKey({ name: "Home", path: "/home/demo", icon: "home" })).toBe(
      "modals:createGroup.suggestionHome",
    );
    expect(suggestionLabelKey({ name: "Current Dir", path: "/srv/app", icon: "current" })).toBe(
      "modals:createGroup.suggestionCurrent",
    );
    for (const suggestion of [
      { name: "Desktop", path: "/home/demo/Desktop", icon: "desktop" },
      { name: "Dev", path: "/home/demo/dev", icon: "folder" },
      { name: "Drive C:", path: "C:\\", icon: "drive" },
    ]) {
      expect(suggestionLabelKey(suggestion)).toBeUndefined();
    }
  });
});
