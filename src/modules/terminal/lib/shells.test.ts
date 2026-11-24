import { describe, expect, it } from "vitest";
import { defaultShellPath, type ShellInfo } from "./shells";

const shells: ShellInfo[] = [
  { name: "PowerShell", path: "C:/pwsh.exe", integrated: true },
  { name: "Command Prompt", path: "C:/cmd.exe", integrated: false },
  { name: "Git Bash", path: "C:/Git/bin/bash.exe", integrated: true },
];

describe("the default shell", () => {
  it("is the setting's when it is installed", () => {
    expect(defaultShellPath(shells, "C:/Git/bin/bash.exe")).toBe(
      "C:/Git/bin/bash.exe",
    );
  });

  it("is the first listed on Auto, or for a shell no longer installed", () => {
    expect(defaultShellPath(shells, "")).toBe("C:/pwsh.exe");
    expect(defaultShellPath(shells, "D:/gone.exe")).toBe("C:/pwsh.exe");
    expect(defaultShellPath([], "")).toBeNull();
  });
});
