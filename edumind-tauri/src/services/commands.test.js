import { describe, expect, it } from "vitest";

import { buildWorkspaceCommands } from "./commands";

describe("buildWorkspaceCommands", () => {
  it("keeps keyboard actions in one schema", async () => {
    const visited = [];
    const commands = buildWorkspaceCommands({
      navigation: [
        { id: "study", label: "Study Review" },
        { id: "admin", label: "Admin" }
      ],
      navigate: (view) => visited.push(view),
      reconnect: () => visited.push("reconnect"),
      refreshStudyInsights: async () => visited.push("refresh")
    });

    expect(commands.map((command) => command.id)).toEqual(expect.arrayContaining([
      "navigate-study",
      "refresh-study-insights",
      "search-memory-evidence",
      "open-active-runs",
      "open-settings",
      "reconnect-gateway"
    ]));

    await commands.find((command) => command.id === "refresh-study-insights").action();
    commands.find((command) => command.id === "open-active-runs").action();

    expect(visited).toEqual(["refresh", "research"]);
  });
});
