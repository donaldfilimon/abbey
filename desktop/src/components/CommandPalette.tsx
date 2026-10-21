import { useEffect, useRef, useState } from "react";
import { SURFACES, type SurfaceId } from "../surfaces";
import { Icon } from "./Icon";
export function CommandPalette({
  close,
  navigate,
}: {
  close: () => void;
  navigate: (id: SurfaceId) => void;
}) {
  const [query, setQuery] = useState("");
  const [selected, setSelected] = useState(0);
  const dialog = useRef<HTMLDivElement>(null);
  const input = useRef<HTMLInputElement>(null);
  const entries = SURFACES.filter((item) =>
    `${item.label} ${item.blurb}`.toLowerCase().includes(query.toLowerCase()),
  );
  useEffect(() => {
    if (entries[selected])
      document
        .getElementById(`command-${entries[selected]!.id}`)
        ?.scrollIntoView({ block: "nearest" });
  }, [selected, query]);
  useEffect(() => {
    const previous = document.activeElement as HTMLElement | null;
    input.current?.focus();
    return () => {
      previous?.focus();
    };
  }, []);
  return (
    <div
      className="palette-backdrop"
      onMouseDown={(event) => {
        if (event.target === event.currentTarget) close();
      }}
    >
      <div
        className="palette"
        role="dialog"
        aria-modal="true"
        aria-label="Navigate Abbey"
        ref={dialog}
        onKeyDown={(event) => {
          if (event.key === "Escape") {
            event.preventDefault();
            close();
          }
          if (event.key === "ArrowDown" || event.key === "ArrowUp") {
            event.preventDefault();
            setSelected((n) =>
              entries.length
                ? (n + (event.key === "ArrowDown" ? 1 : -1) + entries.length) %
                  entries.length
                : 0,
            );
            input.current?.focus();
          }
          if (
            event.key === "Enter" &&
            event.target === input.current &&
            entries[selected]
          ) {
            event.preventDefault();
            navigate(entries[selected]!.id);
            close();
          }
          if (event.key === "Tab") {
            const items = Array.from(
              dialog.current?.querySelectorAll<HTMLElement>("input,button") ??
                [],
            );
            const first = items[0],
              last = items.at(-1);
            if (event.shiftKey && document.activeElement === first) {
              event.preventDefault();
              last?.focus();
            } else if (!event.shiftKey && document.activeElement === last) {
              event.preventDefault();
              first?.focus();
            }
          }
        }}
      >
        <div className="palette-search">
          <Icon name="search" />
          <input
            ref={input}
            role="combobox"
            aria-autocomplete="list"
            aria-expanded="true"
            aria-haspopup="listbox"
            aria-label="Search pages"
            aria-controls="palette-results"
            aria-activedescendant={
              entries[selected] ? `command-${entries[selected]!.id}` : undefined
            }
            placeholder="Where would you like to go?"
            value={query}
            onChange={(event) => {
              setQuery(event.target.value);
              setSelected(0);
            }}
          />
          <button onClick={close} aria-label="Close command palette">
            Esc
          </button>
        </div>
        <div
          id="palette-results"
          role="listbox"
          aria-label="Pages"
          className="palette-results"
        >
          {entries.length === 0 ? (
            <p className="muted">No pages found. Try another search.</p>
          ) : (
            entries.map((entry, index) => (
              <div
                role="option"
                aria-selected={selected === index}
                onMouseDown={(event) => event.preventDefault()}
                id={`command-${entry.id}`}
                className={selected === index ? "command selected" : "command"}
                key={entry.id}
                onClick={() => {
                  navigate(entry.id);
                  close();
                }}
              >
                <Icon name={entry.id} />
                <span>{entry.label}</span>
                <small>Go to page</small>
              </div>
            ))
          )}
        </div>
        <div className="palette-hint">
          ↑ ↓ to navigate <span>↵ to open</span>
        </div>
      </div>
    </div>
  );
}
