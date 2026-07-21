import { useEffect, useMemo, useRef, useState } from "react";

export function CommandPalette({ open, commands, onClose, onError }) {
  const [query, setQuery] = useState("");
  const [activeIndex, setActiveIndex] = useState(0);
  const inputRef = useRef(null);
  const matches = useMemo(() => {
    const normalized = query.trim().toLowerCase();
    return commands.filter((command) => {
      const searchable = [
        command.label,
        command.detail ?? "",
        command.category ?? "",
        ...(command.keywords ?? [])
      ].join(" ").toLowerCase();
      return !normalized || searchable.includes(normalized);
    });
  }, [commands, query]);

  useEffect(() => {
    if (!open) {
      setQuery("");
      setActiveIndex(0);
      return;
    }
    inputRef.current?.focus();
  }, [open]);

  useEffect(() => {
    setActiveIndex((current) => Math.min(current, Math.max(matches.length - 1, 0)));
  }, [matches.length]);

  useEffect(() => {
    if (!open) {
      return undefined;
    }
    const onKeyDown = (event) => {
      if (event.key === "Escape") {
        event.preventDefault();
        onClose();
      }
    };
    window.addEventListener("keydown", onKeyDown);
    return () => window.removeEventListener("keydown", onKeyDown);
  }, [onClose, open]);

  if (!open) {
    return null;
  }

  async function run(command) {
    try {
      await command.action();
      onClose();
    } catch (error) {
      onError?.(error);
    }
  }

  function handleInputKeyDown(event) {
    if (event.key === "ArrowDown" && matches.length) {
      event.preventDefault();
      setActiveIndex((current) => (current + 1) % matches.length);
      return;
    }
    if (event.key === "ArrowUp" && matches.length) {
      event.preventDefault();
      setActiveIndex((current) => (current - 1 + matches.length) % matches.length);
      return;
    }
    if (event.key === "Enter" && matches[activeIndex]) {
      event.preventDefault();
      void run(matches[activeIndex]);
    }
  }

  return (
    <div className="command-palette-backdrop" role="presentation" onMouseDown={onClose}>
      <section
        className="command-palette"
        role="dialog"
        aria-modal="true"
        aria-labelledby="command-palette-heading"
        onMouseDown={(event) => event.stopPropagation()}
      >
        <div className="command-palette-heading">
          <h2 id="command-palette-heading">Command palette</h2>
          <kbd>Esc</kbd>
        </div>
        <label>
          <span>Find a workspace action</span>
          <input
            ref={inputRef}
            value={query}
            onChange={(event) => {
              setQuery(event.target.value);
              setActiveIndex(0);
            }}
            onKeyDown={handleInputKeyDown}
            placeholder="Search navigation and local actions"
            aria-describedby="command-palette-hint"
          />
        </label>
        <p id="command-palette-hint" className="muted command-palette-hint">
          Use the arrow keys and Enter to run the highlighted action.
        </p>
        <div className="command-list" role="list">
          {matches.map((command, index) => (
            <button
              type="button"
              key={command.id}
              className={index === activeIndex ? "active" : ""}
              onClick={() => void run(command)}
              onMouseEnter={() => setActiveIndex(index)}
            >
              <strong>{command.label}</strong>
              {command.detail && <span>{command.detail}</span>}
              {command.category && <small>{command.category}</small>}
            </button>
          ))}
          {!matches.length && <p className="muted">No matching workspace action.</p>}
        </div>
      </section>
    </div>
  );
}
