import { useCallback, useEffect, useRef, useState } from "react";

const RECONNECT_DELAY_MS = 1500;
const POLL_INTERVAL_MS = 30000;

export function useRunTimeline(client, runId) {
  const [state, setState] = useState({
    loading: Boolean(client && runId),
    timeline: [],
    evidence: null,
    error: null,
    connection: client && runId ? "connecting" : "idle"
  });
  const refreshRef = useRef(null);

  const refresh = useCallback(async () => {
    if (!client || !runId) {
      setState({ loading: false, timeline: [], evidence: null, error: null, connection: "idle" });
      return null;
    }
    setState((current) => ({ ...current, loading: true, error: null }));
    try {
      const [timeline, evidence] = await Promise.all([
        client.runTimeline(runId),
        client.runEvidence(runId)
      ]);
      setState((current) => ({ ...current, loading: false, timeline, evidence, error: null }));
      return { timeline, evidence };
    } catch (error) {
      setState((current) => ({ ...current, loading: false, error }));
      return null;
    }
  }, [client, runId]);

  refreshRef.current = refresh;

  useEffect(() => {
    if (!client || !runId) {
      void refresh();
      return undefined;
    }
    let stopped = false;
    let unsubscribe = () => {};
    let reconnectTimer = null;
    let reconnectScheduled = false;

    const scheduleReconnect = () => {
      if (stopped || reconnectScheduled) {
        return;
      }
      reconnectScheduled = true;
      reconnectTimer = window.setTimeout(() => {
        reconnectScheduled = false;
        connect();
      }, RECONNECT_DELAY_MS);
    };

    const connect = () => {
      unsubscribe();
      unsubscribe = client.subscribeEvents({
        onEvent: (event) => {
          const eventRunId = event.payload?.run_id ?? event.payload?.runId;
          if (event.event === "events_lagged" || String(eventRunId ?? "") === String(runId)) {
            void refreshRef.current?.();
          }
        },
        onStatus: (connection) => {
          if (!stopped) {
            setState((current) => ({ ...current, connection }));
          }
          if (connection === "closed" || connection === "error") {
            scheduleReconnect();
          }
        }
      });
    };

    void refresh();
    connect();
    const poller = window.setInterval(() => void refreshRef.current?.(), POLL_INTERVAL_MS);
    return () => {
      stopped = true;
      window.clearInterval(poller);
      if (reconnectTimer) {
        window.clearTimeout(reconnectTimer);
      }
      unsubscribe();
    };
  }, [client, refresh, runId]);

  return { ...state, refresh };
}
