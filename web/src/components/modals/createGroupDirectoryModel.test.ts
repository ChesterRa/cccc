import { describe, expect, it } from "vite-plus/test";
import { directoryNameFromPath, driveSuggestions } from "./createGroupDirectoryModel";

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
});
