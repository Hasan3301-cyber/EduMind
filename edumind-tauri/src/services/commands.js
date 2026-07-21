export function buildWorkspaceCommands({
  navigation,
  navigate,
  reconnect,
  refreshStudyInsights
}) {
  const navigationCommands = navigation.map((item) => ({
    id: `navigate-${item.id}`,
    label: `Open ${item.label}`,
    detail: "Navigate to this workspace",
    category: "Navigation",
    keywords: [item.id, item.label],
    action: () => navigate(item.id)
  }));

  const workspaceCommands = [
    {
      id: "search-memory-evidence",
      label: "Search memory evidence",
      detail: "Open source-linked concepts and retrieval results",
      category: "Search",
      keywords: ["search", "evidence", "sources", "memory"],
      action: () => navigate("memory")
    },
    {
      id: "open-active-runs",
      label: "Open active runs",
      detail: "Inspect recoverable timelines and evidence",
      category: "Runs",
      keywords: ["runs", "timeline", "cancel", "evidence"],
      action: () => navigate("research")
    },
    {
      id: "open-settings",
      label: "Open settings",
      detail: "Review local runtime and integration settings",
      category: "Settings",
      keywords: ["settings", "admin", "runtime", "privacy"],
      action: () => navigate("admin")
    },
    {
      id: "reconnect-gateway",
      label: "Reconnect local gateway",
      detail: "Retry the embedded gateway connection",
      category: "Recovery",
      keywords: ["reconnect", "offline", "gateway"],
      action: reconnect
    }
  ];

  if (refreshStudyInsights) {
    workspaceCommands.unshift({
      id: "refresh-study-insights",
      label: "Refresh study insights",
      detail: "Recompute deterministic recommendations from local data",
      category: "Study",
      keywords: ["study", "review", "recommendations", "refresh"],
      action: refreshStudyInsights
    });
  }

  return [...navigationCommands, ...workspaceCommands];
}
