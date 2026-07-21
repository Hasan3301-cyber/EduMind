import { useCallback, useEffect, useMemo, useState } from "react";

import { AdminPanel } from "./components/AdminPanel";
import { ChatPanel } from "./components/ChatPanel";
import { CommandPalette } from "./components/shared/CommandPalette";
import { ConnectionModal } from "./components/ConnectionModal";
import { ErrorBoundary } from "./components/ErrorBoundary";
import { GroupStudyPanel } from "./components/GroupStudyPanel";
import { MemoryGraphPanel } from "./components/MemoryGraphPanel";
import { ModulePanel } from "./components/ModulePanel";
import { ResearchWorkspace } from "./components/ResearchWorkspace";
import { RoutineManagerPanel } from "./components/RoutineManagerPanel";
import { Sidebar } from "./components/Sidebar";
import { SrsReviewPanel } from "./components/SrsReviewPanel";
import { StudentOSPage } from "./components/StudentOSPage";
import { StudentPlannerPage } from "./components/StudentPlannerPage";
import { TaskAutomationPanel } from "./components/TaskAutomationPanel";
import { WellnessPanel } from "./components/WellnessPanel";
import { WorkflowWorkspace } from "./components/WorkflowWorkspace";
import { buildWorkspaceCommands } from "./services/commands";
import { GatewayClient } from "./services/gateway";
import { getGatewayEndpoint } from "./tauri-bridge";
import "./reference-ui.css";

const THEME_STORAGE_KEY = "edumind-theme";

const NAVIGATION = [
  { id: "home", label: "Home", icon: "fa-house" },
  { id: "class-notes", label: "Notes", icon: "fa-note-sticky" },
  { id: "exam-practice", label: "Exam Practice", icon: "fa-pen-ruler" },
  { id: "research", label: "Research", icon: "fa-flask" },
  { id: "study", label: "Review", icon: "fa-brain" },
  { id: "group-study", label: "Study Groups", icon: "fa-people-group" },
  { id: "student-os", label: "Student Hub", icon: "fa-graduation-cap" },
  { id: "planner", label: "Planner", icon: "fa-calendar-days" },
  { id: "routine", label: "Routine Coach", icon: "fa-clock" },
  { id: "wellness", label: "Wellbeing", icon: "fa-heart-pulse" },
  { id: "automation", label: "Smart Tasks", icon: "fa-list-check" },
  { id: "memory", label: "Learning Map", icon: "fa-diagram-project" },
  { id: "chat", label: "AI Tutor", icon: "fa-message" },
  { id: "admin", label: "Settings", icon: "fa-sliders" }
];

function getInitialTheme() {
  if (typeof window === "undefined") {
    return "light";
  }

  try {
    const savedTheme = window.localStorage.getItem(THEME_STORAGE_KEY);
    if (savedTheme === "light" || savedTheme === "dark") {
      return savedTheme;
    }
  } catch {
    // Storage can be unavailable in privacy-restricted webviews.
  }

  return window.matchMedia?.("(prefers-color-scheme: dark)")?.matches ? "dark" : "light";
}

export default function App() {
  const [activeView, setActiveView] = useState("home");
  const [theme, setTheme] = useState(getInitialTheme);
  const [endpoint, setEndpoint] = useState(null);
  const [client, setClient] = useState(null);
  const [connectionState, setConnectionState] = useState("connecting");
  const [connectionError, setConnectionError] = useState(null);
  const [connectionOpen, setConnectionOpen] = useState(true);
  const [commandOpen, setCommandOpen] = useState(false);

  const connect = useCallback(async () => {
    setConnectionState("connecting");
    setConnectionError(null);
    try {
      const nextEndpoint = await getGatewayEndpoint();
      setEndpoint(nextEndpoint);
      if (!nextEndpoint.baseUrl) {
        setClient(null);
        setConnectionState("offline");
        return;
      }
      const nextClient = new GatewayClient(nextEndpoint);
      await nextClient.health();
      setClient(nextClient);
      setConnectionState("connected");
    } catch (error) {
      setClient(null);
      setConnectionState("offline");
      setConnectionError(error.message);
    }
  }, []);

  useEffect(() => {
    void connect();
  }, [connect]);

  useEffect(() => {
    document.documentElement.dataset.theme = theme;
    try {
      window.localStorage.setItem(THEME_STORAGE_KEY, theme);
    } catch {
      // Keep theme switching functional when persistence is unavailable.
    }
  }, [theme]);

  useEffect(() => {
    const onKeyDown = (event) => {
      if ((event.metaKey || event.ctrlKey) && event.key.toLowerCase() === "k") {
        event.preventDefault();
        setCommandOpen(true);
      }
    };
    window.addEventListener("keydown", onKeyDown);
    return () => window.removeEventListener("keydown", onKeyDown);
  }, []);

  const commands = useMemo(() => {
    return buildWorkspaceCommands({
      navigation: NAVIGATION,
      navigate: setActiveView,
      reconnect: connect,
      refreshStudyInsights: client
        ? async () => {
          await client.refreshStudyRecommendations();
          setActiveView("study");
        }
        : undefined
    });
  }, [client, connect]);

  const activeNavigationItem = NAVIGATION.find((item) => item.id === activeView) ?? NAVIGATION[0];

  return (
    <ErrorBoundary>
      <div className="app-shell">
        <Sidebar
          items={NAVIGATION}
          activeView={activeView}
          onNavigate={setActiveView}
          theme={theme}
          onThemeToggle={() => setTheme((current) => (current === "dark" ? "light" : "dark"))}
          connectionState={connectionState}
        />
        <main className="main-content">
          <header className="workspace-chrome">
            <div className="workspace-context">
              <span className="workspace-kicker">EduMind AI</span>
              <strong>{activeNavigationItem.label}</strong>
            </div>
            <button type="button" className="command-palette-trigger" onClick={() => setCommandOpen(true)} aria-label="Open command palette">
              <i className="fa-solid fa-magnifying-glass" aria-hidden="true" />
              <span>Commands</span>
              <kbd>Ctrl K</kbd>
            </button>
          </header>
          {renderView(activeView, {
            client,
            endpoint,
            connectionState,
            onNavigate: setActiveView
          })}
        </main>
      </div>
      <ConnectionModal
        open={connectionOpen}
        endpoint={endpoint}
        status={connectionState}
        error={connectionError}
        onClose={() => setConnectionOpen(false)}
        onRetry={() => void connect()}
      />
      <CommandPalette
        open={commandOpen}
        commands={commands}
        onClose={() => setCommandOpen(false)}
        onError={(error) => setConnectionError(error.message)}
      />
    </ErrorBoundary>
  );
}

function renderView(view, context) {
  switch (view) {
    case "class-notes":
      return <WorkflowWorkspace kind="class-notes" client={context.client} connectionState={context.connectionState} onNavigate={context.onNavigate} />;
    case "exam-practice":
      return <WorkflowWorkspace kind="exam-practice" client={context.client} connectionState={context.connectionState} onNavigate={context.onNavigate} />;
    case "research":
      return <ResearchWorkspace client={context.client} />;
    case "study":
      return <SrsReviewPanel client={context.client} />;
    case "group-study":
      return <GroupStudyPanel client={context.client} connectionState={context.connectionState} onNavigate={context.onNavigate} />;
    case "student-os":
      return <StudentOSPage client={context.client} />;
    case "planner":
      return <StudentPlannerPage client={context.client} connectionState={context.connectionState} />;
    case "routine":
      return <RoutineManagerPanel client={context.client} connectionState={context.connectionState} onNavigate={context.onNavigate} />;
    case "wellness":
      return <WellnessPanel client={context.client} connectionState={context.connectionState} />;
    case "automation":
      return <TaskAutomationPanel client={context.client} connectionState={context.connectionState} onNavigate={context.onNavigate} />;
    case "memory":
      return <MemoryGraphPanel client={context.client} />;
    case "chat":
      return <ChatPanel client={context.client} connectionState={context.connectionState} onNavigate={context.onNavigate} />;
    case "admin":
      return <AdminPanel client={context.client} endpoint={context.endpoint} />;
    default:
      return <ModulePanel onNavigate={context.onNavigate} client={context.client} connectionState={context.connectionState} />;
  }
}
