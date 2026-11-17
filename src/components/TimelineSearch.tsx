import { useEffect, useLayoutEffect, useMemo, useRef, useState, type RefObject } from "react";
import { createPortal } from "react-dom";
import type { LibraryTrack } from "../library/types";
import type { TimelineClip } from "../timeline/types";
import { searchTimeline } from "../lib/timelineSearch";
import { laneLetter } from "../lib/timelineLane";

interface TimelineSearchProps {
  clips: readonly TimelineClip[];
  tracks: readonly LibraryTrack[];
  anchor: RefObject<HTMLButtonElement | null>;
  onChoose: (clipId: number) => void;
  onClose: (restoreFocus: boolean) => void;
}

export function TimelineSearch({ clips, tracks, anchor, onChoose, onClose }: TimelineSearchProps) {
  const panel = useRef<HTMLDivElement>(null);
  const input = useRef<HTMLInputElement>(null);
  const [query, setQuery] = useState("");
  const [activeIndex, setActiveIndex] = useState(0);
  const [position, setPosition] = useState({ left: 8, top: 8 });
  const results = useMemo(() => searchTimeline(clips, tracks, query), [clips, tracks, query]);
  const active = Math.min(activeIndex, Math.max(0, results.length - 1));

  useLayoutEffect(() => {
    const place = () => {
      const button = anchor.current?.getBoundingClientRect();
      const bounds = panel.current?.getBoundingClientRect();
      if (!button || !bounds) return;
      setPosition({ left: Math.max(8, Math.min(button.left, window.innerWidth - bounds.width - 8)),
        top: Math.max(8, Math.min(button.bottom + 8, window.innerHeight - bounds.height - 8)) });
    };
    place();
    window.addEventListener("resize", place);
    return () => window.removeEventListener("resize", place);
  }, [anchor, results.length]);

  useEffect(() => { input.current?.focus(); }, []);
  useEffect(() => {
    panel.current?.querySelector<HTMLElement>(`[data-result-index="${active}"]`)?.scrollIntoView({ block: "nearest" });
  }, [active, query]);
  useEffect(() => {
    const closeOutside = (event: PointerEvent) => {
      if (event.target instanceof Node && !panel.current?.contains(event.target) && !anchor.current?.contains(event.target)) onClose(false);
    };
    document.addEventListener("pointerdown", closeOutside, true);
    return () => document.removeEventListener("pointerdown", closeOutside, true);
  }, [anchor, onClose]);

  return createPortal(
    <div ref={panel} className="timeline-search" style={position} role="dialog" aria-label="Search timeline"
      onBlur={(event) => {
        if (event.relatedTarget instanceof Node && !panel.current?.contains(event.relatedTarget)
          && !anchor.current?.contains(event.relatedTarget)) onClose(false);
      }}
      onKeyDown={(event) => {
        event.stopPropagation();
        if (event.key === "Escape") { event.preventDefault(); onClose(true); }
      }}>
      <div className="timeline-search-heading">
        <label htmlFor="timeline-search-input">Find a song in the timeline</label>
        <button type="button" aria-label="Close timeline search" onClick={() => onClose(true)}>×</button>
      </div>
      <input ref={input} id="timeline-search-input" type="search" value={query} placeholder="Title, artist or filename…"
        autoComplete="off" role="combobox" aria-autocomplete="list" aria-expanded="true"
        aria-controls="timeline-search-results" aria-activedescendant={results.length ? `timeline-search-result-${results[active].clip.id}` : undefined}
        onChange={(event) => { setQuery(event.target.value); setActiveIndex(0); }}
        onKeyDown={(event) => {
          if (event.key === "ArrowDown" || event.key === "ArrowUp") {
            event.preventDefault();
            setActiveIndex(Math.max(0, Math.min(results.length - 1, active + (event.key === "ArrowDown" ? 1 : -1))));
          } else if (event.key === "Enter") {
            event.preventDefault();
            if (results[active]) onChoose(results[active].clip.id);
          }
        }} />
      <p className="timeline-search-count" role="status">{results.length} {results.length === 1 ? "clip" : "clips"} found</p>
      <div className="timeline-search-results" id="timeline-search-results" role="listbox" aria-label="Timeline clips">
        {results.map(({ clip, name, sequence, bar }, index) => (
          <button type="button" role="option" aria-selected={index === active} data-result-index={index}
            id={`timeline-search-result-${clip.id}`} key={clip.id} title={name}
            onFocus={() => setActiveIndex(index)} onClick={() => onChoose(clip.id)}>
            <strong>{name}</strong>
            <span>#{sequence} · Track {laneLetter(clip.lane)} · Bar {bar}</span>
          </button>
        ))}
      </div>
      {results.length === 0 && <p className="timeline-search-empty">No song in this timeline matches your search.</p>}
      <p className="timeline-search-hint">Select a clip to locate it. Playback stays unchanged.</p>
    </div>, document.body,
  );
}
