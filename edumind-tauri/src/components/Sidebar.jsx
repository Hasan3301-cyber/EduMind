import { useState } from "react";

const NAVIGATION_GROUPS = [
  ["home", "student-os", "planner", "study"],
  ["admin", "automation"],
  ["class-notes", "exam-practice", "group-study", "routine", "research", "wellness", "memory", "chat"]
];

export function Sidebar({ items, activeView, onNavigate, theme, onThemeToggle, connectionState }) {
  const [expanded, setExpanded] = useState(false);
  const itemById = new Map(items.map((item) => [item.id, item]));
  const groupedItems = NAVIGATION_GROUPS
    .map((group) => group.map((id) => itemById.get(id)).filter(Boolean))
    .filter((group) => group.length);
  const groupedIds = new Set(NAVIGATION_GROUPS.flat());
  const remainingItems = items.filter((item) => !groupedIds.has(item.id));
  if (remainingItems.length) {
    groupedItems.push(remainingItems);
  }
  const connectionLabel = connectionState === "connected" ? "Embedded gateway online" : "Offline preview";

  return (
    <aside className={expanded ? "sidebar is-expanded" : "sidebar"} aria-label="EduMind navigation">
      <div className="brand-lockup">
        <span className="brand-mark" aria-hidden="true"><i className="fa-solid fa-brain" /></span>
        {expanded && (
          <div className="brand-copy">
            <strong>EduMind</strong>
            <span>Local-first learning OS</span>
          </div>
        )}
      </div>
      <nav className="sidebar-nav" aria-label="Learning spaces">
        {groupedItems.map((group, groupIndex) => (
          <div className="nav-section" key={`navigation-group-${groupIndex}`}>
            {group.map((item) => (
              <button
                type="button"
                key={item.id}
                className={item.id === activeView ? "nav-button active" : "nav-button"}
                aria-label={item.label}
                aria-current={item.id === activeView ? "page" : undefined}
                title={item.label}
                onClick={() => onNavigate(item.id)}
              >
                <i className={`fa-solid ${item.icon}`} aria-hidden="true" />
                {expanded && <span>{item.label}</span>}
              </button>
            ))}
          </div>
        ))}
      </nav>
      <div className="sidebar-footer">
        <div className="connection-status" aria-label={connectionLabel} title={connectionLabel}>
          <span className={`connection-dot ${connectionState}`} aria-hidden="true" />
          {expanded && <span>{connectionLabel}</span>}
        </div>
        <button
          type="button"
          className="theme-button"
          aria-label={`Switch to ${theme === "dark" ? "light" : "dark"} theme`}
          aria-pressed={theme === "dark"}
          onClick={onThemeToggle}
        >
          <i className={`fa-solid ${theme === "dark" ? "fa-sun" : "fa-moon"}`} aria-hidden="true" />
        </button>
      </div>
      <button
        type="button"
        className="sidebar-toggle-button"
        aria-label={expanded ? "Collapse navigation" : "Expand navigation"}
        title={expanded ? "Collapse navigation" : "Expand navigation"}
        onClick={() => setExpanded((current) => !current)}
      >
        <i className={`fa-solid ${expanded ? "fa-chevron-left" : "fa-chevron-right"}`} aria-hidden="true" />
      </button>
    </aside>
  );
}
