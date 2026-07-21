export function ConnectionModal({ open, endpoint, status, error, onClose, onRetry }) {
  if (!open) {
    return null;
  }
  return (
    <div className="modal-backdrop" role="presentation">
      <section className="connection-modal" role="dialog" aria-modal="true" aria-labelledby="connection-title">
        <p className="eyebrow">Embedded gateway</p>
        <h2 id="connection-title">{status === "connected" ? "Ready to learn" : "Starting local workspace"}</h2>
        <p>
          EduMind runs its gateway in this desktop app. Your data remains on this device unless you
          choose an external integration.
        </p>
        {endpoint?.embedded && <p className="muted">Authenticated loopback service is active.</p>}
        {endpoint?.baseUrl && !endpoint.embedded && <p className="muted">Browser preview needs a separately running EduMind gateway. The Vite frontend URL is not a gateway API.</p>}
        {error && <p className="error-message">{error}</p>}
        <div className="modal-actions">
          {status !== "connected" && (
            <button type="button" onClick={onRetry}>Retry gateway check</button>
          )}
          <button type="button" className="secondary-button" onClick={onClose}>
            Continue offline
          </button>
        </div>
      </section>
    </div>
  );
}
