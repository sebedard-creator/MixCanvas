import React, { useState, useEffect, useRef } from "react";
import { TimelineClip, ClipEqSettings } from "../timeline/types";
import {
  CLIP_EQ_GAIN_MAX_DB,
  CLIP_EQ_MAX_FREQ_HZ,
  CLIP_EQ_MIN_FREQ_HZ,
  CLIP_EQ_PEAK_MAX_DB,
  CLIP_EQ_SILENCE_DB,
  DEFAULT_CLIP_EQ,
  isClipEqSilent,
  parseClipEqGainDb,
  sanitizeClipEq,
} from "../lib/clipEq";

interface ClipEqModalProps {
  clip: TimelineClip;
  onClose: () => void;
  onSave: (clipId: number, eqSettings: ClipEqSettings) => void;
}

/** Delay before a slider gesture reaches SQLite and the audio engine. */
const LIVE_SAVE_DEBOUNCE_MS = 200;

const MIN_FREQ = CLIP_EQ_MIN_FREQ_HZ;
const MAX_FREQ = CLIP_EQ_MAX_FREQ_HZ;
const LOG_RATIO = Math.log10(MAX_FREQ / MIN_FREQ);

function freqToX(freq: number, width: number): number {
  const clamped = Math.max(MIN_FREQ, Math.min(MAX_FREQ, freq));
  return (Math.log10(clamped / MIN_FREQ) / LOG_RATIO) * width;
}

function xToFreq(x: number, width: number): number {
  const normalized = Math.max(0, Math.min(1, x / width));
  const freq = MIN_FREQ * Math.pow(10, normalized * LOG_RATIO);
  return Math.round(freq);
}

/** Floor of the drawn response curve; the parameters themselves go lower. */
const MIN_DB = -36;
const MAX_DB = 6;
const SLIDER_MIN_DB = CLIP_EQ_SILENCE_DB; // reaching the floor means -∞ dB
const GRAPH_WIDTH = 580;
const GRAPH_HEIGHT = 220;
const ZERO_Y = 48; // 0 dB reference line Y position
const DB_SCALE = 3.6; // pixels per dB

function dbToY(db: number): number {
  const clamped = Math.max(MIN_DB, Math.min(MAX_DB, db));
  return ZERO_Y - clamped * DB_SCALE;
}

function yToDb(y: number): number {
  const rawDb = (ZERO_Y - y) / DB_SCALE;
  if (rawDb <= MIN_DB) return CLIP_EQ_SILENCE_DB; // dragged to the floor: full cut
  return Math.round(Math.max(MIN_DB, Math.min(MAX_DB, rawDb)));
}

function formatGainDisplay(db: number): string {
  if (isClipEqSilent(db)) return "-∞";
  if (db > 0) return `+${db}`;
  return `${db}`;
}

// HPF magnitude calculation
function calcHpDb(f: number, fc: number): number {
  if (fc <= MIN_FREQ) return 0;
  const ratio = f / fc;
  const mag = ratio * ratio / Math.sqrt(1 + ratio * ratio * ratio * ratio);
  return Math.max(MIN_DB, 20 * Math.log10(Math.max(0.001, mag)));
}

// LPF magnitude calculation
function calcLpDb(f: number, fc: number): number {
  if (fc >= MAX_FREQ) return 0;
  const ratio = f / fc;
  const mag = 1 / Math.sqrt(1 + ratio * ratio * ratio * ratio);
  return Math.max(MIN_DB, 20 * Math.log10(Math.max(0.001, mag)));
}

// 3rd Parametric Bell EQ magnitude calculation (supports cut down to -∞ dB / notch)
function calcPeakDb(f: number, fc: number, gainDb: number, q: number): number {
  if (gainDb === 0) return 0;
  const oct = Math.abs(Math.log2(f / fc));
  const bandwidth = 1 / Math.max(0.1, q);
  const factor = Math.exp(-Math.pow(oct / bandwidth, 2) * 2.2);

  if (isClipEqSilent(gainDb)) {
    // Deep notch / -∞ dB attenuation near cutoff frequency
    return MIN_DB * factor;
  }
  return Math.max(MIN_DB, gainDb) * factor;
}

export const ClipEqModal: React.FC<ClipEqModalProps> = ({ clip, onClose, onSave }) => {
  const initialSettings: ClipEqSettings = sanitizeClipEq(clip.eqSettings);

  const [hpHz, setHpHz] = useState<number>(initialSettings.highPassHz);
  const [lpHz, setLpHz] = useState<number>(initialSettings.lowPassHz);
  const [peakHz, setPeakHz] = useState<number>(initialSettings.peakHz ?? 1000);
  const [peakGainDb, setPeakGainDb] = useState<number>(initialSettings.peakGainDb ?? 0);
  const [peakQ, setPeakQ] = useState<number>(initialSettings.peakQ ?? 1.0);
  const [gainDb, setGainDb] = useState<number>(initialSettings.gainDb ?? 0);
  /**
   * Ce qui est écrit dans la case tant qu'on n'a pas validé.
   *
   * `null` quand personne n'y touche : le champ montre alors la valeur réelle,
   * et suit donc le curseur. Le brouillon n'existe que le temps d'une saisie.
   */
  const [gainDraft, setGainDraft] = useState<string | null>(null);

  /** Valide la saisie, ou la rejette sans rien changer. */
  const commitGain = () => {
    if (gainDraft === null) return;
    const parsed = parseClipEqGainDb(gainDraft);
    setGainDraft(null);
    // Une saisie qui ne veut rien dire laisse le réglage tel quel : mieux vaut
    // ne rien faire que couper un clip sur une frappe malheureuse.
    if (parsed !== null) setGainDb(parsed);
  };
  const [enabled, setEnabled] = useState<boolean>(initialSettings.enabled ?? true);

  const [activeDrag, setActiveDrag] = useState<"hp" | "lp" | "peak" | null>(null);

  const svgRef = useRef<SVGSVGElement | null>(null);
  const isFirstRender = useRef(true);

  // `onSave` is read through a ref so it never appears in the live-save effect's
  // dependencies. It used to: the parent rebuilt the callback on every timeline
  // snapshot, so each save re-fired the effect that produced it — an endless
  // write loop that also flooded the undo history.
  const saveRef = useRef(onSave);
  saveRef.current = onSave;

  // Sync state if clip changes
  useEffect(() => {
    const s = sanitizeClipEq(clip.eqSettings);
    isFirstRender.current = true;
    setHpHz(s.highPassHz);
    setLpHz(s.lowPassHz);
    setPeakHz(s.peakHz ?? 1000);
    setPeakGainDb(s.peakGainDb ?? 0);
    setPeakQ(s.peakQ ?? 1.0);
    setGainDb(s.gainDb ?? 0);
    setEnabled(s.enabled ?? true);
  }, [clip]);

  // Live audio update. Debounced so that dragging a slider does not rewrite
  // SQLite and rebuild the whole playback plan on every pointer sample.
  useEffect(() => {
    if (isFirstRender.current) {
      isFirstRender.current = false;
      return undefined;
    }
    const timer = window.setTimeout(() => {
      saveRef.current(
        clip.id,
        sanitizeClipEq({
          highPassHz: hpHz,
          lowPassHz: lpHz,
          peakHz,
          peakGainDb,
          peakQ,
          gainDb,
          enabled,
        }),
      );
    }, LIVE_SAVE_DEBOUNCE_MS);
    return () => window.clearTimeout(timer);
  }, [clip.id, hpHz, lpHz, peakHz, peakGainDb, peakQ, gainDb, enabled]);

  const handleReset = () => {
    setHpHz(DEFAULT_CLIP_EQ.highPassHz);
    setLpHz(DEFAULT_CLIP_EQ.lowPassHz);
    setPeakHz(DEFAULT_CLIP_EQ.peakHz ?? 1000);
    setPeakGainDb(DEFAULT_CLIP_EQ.peakGainDb ?? 0);
    setPeakQ(DEFAULT_CLIP_EQ.peakQ ?? 1.0);
    setGainDb(DEFAULT_CLIP_EQ.gainDb ?? 0);
    setEnabled(DEFAULT_CLIP_EQ.enabled ?? true);
  };

  // Generate frequency response curve SVG path
  const points: string[] = [];
  const steps = 140;
  for (let i = 0; i <= steps; i++) {
    const x = (i / steps) * GRAPH_WIDTH;
    const f = xToFreq(x, GRAPH_WIDTH);
    const hpDb = enabled ? calcHpDb(f, hpHz) : 0;
    const lpDb = enabled ? calcLpDb(f, lpHz) : 0;
    const pkDb = enabled ? calcPeakDb(f, peakHz, peakGainDb, peakQ) : 0;
    const clipGainOffset = enabled ? Math.max(MIN_DB, gainDb) : 0;
    const totalDb = Math.max(MIN_DB, Math.min(MAX_DB, hpDb + lpDb + pkDb + clipGainOffset));
    const y = dbToY(totalDb);
    points.push(`${x.toFixed(1)},${y.toFixed(1)}`);
  }
  const curvePath = `M ${points.join(" L ")}`;

  const hpX = freqToX(hpHz, GRAPH_WIDTH);
  const lpX = freqToX(lpHz, GRAPH_WIDTH);
  const peakX = freqToX(peakHz, GRAPH_WIDTH);
  const peakY = dbToY(enabled ? peakGainDb : 0);

  // Pointer drag for EQ handles on graph
  const handlePointerDown = (type: "hp" | "lp" | "peak") => (e: React.PointerEvent) => {
    e.preventDefault();
    (e.target as HTMLElement).setPointerCapture(e.pointerId);
    setActiveDrag(type);
  };

  const handlePointerMove = (e: React.PointerEvent) => {
    if (!activeDrag || !svgRef.current) return;
    // Le graphe se dessine en unités de `viewBox` mais s'affiche à la largeur
    // qu'on lui donne. Sans conversion, la poignée n'allait qu'à une fraction
    // de la distance parcourue par la main — l'effet élastique.
    const { x, y } = graphPointToViewBox(
      e.clientX,
      e.clientY,
      svgRef.current.getBoundingClientRect(),
      GRAPH_WIDTH,
      GRAPH_HEIGHT,
    );

    const freq = xToFreq(x, GRAPH_WIDTH);

    if (activeDrag === "hp") {
      setHpHz(Math.min(freq, lpHz - 50));
    } else if (activeDrag === "lp") {
      setLpHz(Math.max(freq, hpHz + 50));
    } else if (activeDrag === "peak") {
      setPeakHz(freq);
      setPeakGainDb(yToDb(y));
    }
  };

  const handlePointerUp = (e: React.PointerEvent) => {
    if (activeDrag) {
      try {
        (e.target as HTMLElement).releasePointerCapture(e.pointerId);
      } catch {
        // ignore
      }
      setActiveDrag(null);
    }
  };

  const gridFreqs = [50, 100, 250, 500, 1000, 2500, 5000, 10000];

  return (
    <div className="clip-eq-modal-backdrop" onClick={onClose}>
      <div className="clip-eq-modal-dialog" onClick={(e) => e.stopPropagation()}>
        {/* Header */}
        <div className="clip-eq-modal-header">
          <div className="clip-eq-title-group">
            <span className="clip-eq-badge">3-BAND CLIP EQ</span>
            <span className="clip-eq-filename" title={clip.fileName}>
              {clip.fileName}
            </span>
          </div>
          <button className="clip-eq-close-btn" onClick={onClose} aria-label="Close" title="Close EQ window">
            ✕
          </button>
        </div>

        {/* LCD Graph Area */}
        <div className="clip-eq-graph-container">
          <svg
            ref={svgRef}
            className="clip-eq-svg"
            viewBox={`0 0 ${GRAPH_WIDTH} ${GRAPH_HEIGHT}`}
            onPointerMove={handlePointerMove}
            onPointerUp={handlePointerUp}
            onPointerCancel={handlePointerUp}
          >
            {/* Background Grid */}
            <rect width={GRAPH_WIDTH} height={GRAPH_HEIGHT} fill="#0f1926" rx="4" />

            {/* +6 dB Line (Gain Boost Limit) */}
            <line x1="0" x2={GRAPH_WIDTH} y1={dbToY(6)} y2={dbToY(6)} stroke="rgba(52, 211, 153, 0.35)" strokeDasharray="3,3" />
            <text x="8" y={dbToY(6) - 3} fill="#34D399" fontSize="9" fontWeight="bold" fontFamily="monospace">
              +6 dB
            </text>

            {/* 0 dB Reference Line */}
            <line x1="0" x2={GRAPH_WIDTH} y1={ZERO_Y} y2={ZERO_Y} stroke="rgba(255, 255, 255, 0.28)" strokeDasharray="4,4" />
            <text x="8" y={ZERO_Y - 3} fill="rgba(255,255,255,0.6)" fontSize="9" fontWeight="bold" fontFamily="monospace">
              0 dB
            </text>

            {/* -12dB, -24dB, -36dB Reference Lines */}
            {[-12, -24, -36].map((db) => {
              const y = dbToY(db);
              return (
                <g key={db}>
                  <line x1="0" x2={GRAPH_WIDTH} y1={y} y2={y} stroke="rgba(255, 255, 255, 0.08)" strokeDasharray="3,3" />
                  <text x="8" y={y - 3} fill="rgba(255,255,255,0.3)" fontSize="9" fontFamily="monospace">
                    {db === MIN_DB ? `${MIN_DB}dB (-∞)` : `${db} dB`}
                  </text>
                </g>
              );
            })}

            {/* Vertical Frequency Grid Lines */}
            {gridFreqs.map((f) => {
              const x = freqToX(f, GRAPH_WIDTH);
              const label = f >= 1000 ? `${f / 1000}k` : `${f}`;
              return (
                <g key={f}>
                  <line x1={x} x2={x} y1="0" y2={GRAPH_HEIGHT} stroke="rgba(255, 255, 255, 0.08)" strokeDasharray="3,3" />
                  <text x={x} y={GRAPH_HEIGHT - 6} fill="rgba(255,255,255,0.35)" fontSize="9" textAnchor="middle" fontFamily="monospace">
                    {label}
                  </text>
                </g>
              );
            })}

            {/* Filled Area under Curve */}
            <path
              d={`${curvePath} L ${GRAPH_WIDTH},${GRAPH_HEIGHT} L 0,${GRAPH_HEIGHT} Z`}
              fill="rgba(14, 165, 233, 0.15)"
            />

            {/* Frequency Response Curve */}
            <path d={curvePath} fill="none" stroke={enabled ? "#0EA5E9" : "#64748B"} strokeWidth="2.5" />

            {/* HPF Cutoff Handle (Blue) */}
            {enabled && (
              <g
                className="eq-handle"
                style={{ cursor: "ew-resize" }}
                onPointerDown={handlePointerDown("hp")}
              >
                <title>{`High-Pass Filter (HPF): ${hpHz} Hz (Drag horizontally to adjust cutoff)`}</title>
                <line x1={hpX} x2={hpX} y1="0" y2={GRAPH_HEIGHT} stroke="#0284C7" strokeWidth="1.5" strokeDasharray="4,4" />
                <circle cx={hpX} cy={ZERO_Y} r="7" fill="#0284C7" stroke="#FFFFFF" strokeWidth="2" />
                <text x={hpX} y="16" fill="#38BDF8" fontSize="9.5" fontWeight="bold" textAnchor="middle" fontFamily="monospace">
                  HPF {hpHz >= 1000 ? `${(hpHz / 1000).toFixed(1)}k` : `${hpHz}`}Hz
                </text>
              </g>
            )}

            {/* 3rd Parametric EQ Bell Handle (Amber/Yellow - Frequency X & Gain Y) */}
            {enabled && (
              <g
                className="eq-handle"
                style={{ cursor: "move" }}
                onPointerDown={handlePointerDown("peak")}
              >
                <title>{`Bell EQ (EQ3): ${peakHz} Hz, ${formatGainDisplay(peakGainDb)} dB, Q ${peakQ} (Drag to adjust frequency & gain)`}</title>
                <line x1={peakX} x2={peakX} y1="0" y2={GRAPH_HEIGHT} stroke="#F59E0B" strokeWidth="1.5" strokeDasharray="3,3" />
                <circle cx={peakX} cy={peakY} r="8" fill="#F59E0B" stroke="#FFFFFF" strokeWidth="2" />
                <text x={peakX} y={Math.max(28, peakY - 12)} fill="#FBBF24" fontSize="10" fontWeight="bold" textAnchor="middle" fontFamily="monospace">
                  EQ3 {peakHz >= 1000 ? `${(peakHz / 1000).toFixed(1)}k` : `${peakHz}`}Hz ({formatGainDisplay(peakGainDb)}dB)
                </text>
              </g>
            )}

            {/* LPF Cutoff Handle (Red/Coral) */}
            {enabled && (
              <g
                className="eq-handle"
                style={{ cursor: "ew-resize" }}
                onPointerDown={handlePointerDown("lp")}
              >
                <title>{`Low-Pass Filter (LPF): ${lpHz} Hz (Drag horizontally to adjust cutoff)`}</title>
                <line x1={lpX} x2={lpX} y1="0" y2={GRAPH_HEIGHT} stroke="#EF4444" strokeWidth="1.5" strokeDasharray="4,4" />
                <circle cx={lpX} cy={ZERO_Y} r="7" fill="#EF4444" stroke="#FFFFFF" strokeWidth="2" />
                <text x={lpX} y="30" fill="#F87171" fontSize="9.5" fontWeight="bold" textAnchor="middle" fontFamily="monospace">
                  LPF {lpHz >= 1000 ? `${(lpHz / 1000).toFixed(1)}k` : `${lpHz}`}Hz
                </text>
              </g>
            )}
          </svg>
        </div>

        {/* Controls Section (3 Cards: HPF, 3rd Parametric EQ, LPF) */}
        <div className="clip-eq-controls-row">
          {/* Card 1: HPF Control */}
          <div className={`clip-eq-param-card ${enabled ? "clip-eq-param-card--hpf" : "clip-eq-param-card--disabled"}`}>
            <div className="clip-eq-param-label">
              <span className="clip-eq-indicator clip-eq-indicator--hpf" />
              HIGH-PASS (HPF)
            </div>
            <div className="clip-eq-field-row">
              <span className="clip-eq-field-title">Cutoff:</span>
              <input
                type="range"
                min={20}
                max={20000}
                step={5}
                value={hpHz}
                disabled={!enabled}
                title="Adjust High-Pass Cutoff Frequency (Hz)"
                onChange={(e) => setHpHz(Math.min(Number(e.target.value), lpHz - 50))}
                className="clip-eq-range clip-eq-range--hpf"
              />
              <div className="clip-eq-value-display" title="High-Pass Cutoff Frequency in Hertz">
                <input
                  type="number"
                  min={20}
                  max={20000}
                  value={hpHz}
                  disabled={!enabled}
                  onChange={(e) => setHpHz(Math.min(Math.max(20, Number(e.target.value)), lpHz - 50))}
                  className="clip-eq-num-input"
                />
                <span className="clip-eq-unit">Hz</span>
              </div>
            </div>
          </div>

          {/* Card 2: 3rd Parametric EQ (Bell / Peaking with Cutoff, Gain [-∞ to +6dB], and Q) */}
          <div className={`clip-eq-param-card ${enabled ? "clip-eq-param-card--peak" : "clip-eq-param-card--disabled"}`}>
            <div className="clip-eq-param-label">
              <span className="clip-eq-indicator clip-eq-indicator--peak" />
              PARAMETRIC BELL (EQ3)
            </div>

            {/* Cutoff Frequency */}
            <div className="clip-eq-field-row">
              <span className="clip-eq-field-title">Frequency:</span>
              <input
                type="range"
                min={20}
                max={20000}
                step={10}
                value={peakHz}
                disabled={!enabled}
                title="Adjust Bell EQ Center Frequency (Hz)"
                onChange={(e) => setPeakHz(Number(e.target.value))}
                className="clip-eq-range clip-eq-range--peak"
              />
              <div className="clip-eq-value-display" title="Bell EQ Center Frequency in Hertz">
                <input
                  type="number"
                  min={20}
                  max={20000}
                  value={peakHz}
                  disabled={!enabled}
                  onChange={(e) => setPeakHz(Math.max(20, Math.min(20000, Number(e.target.value))))}
                  className="clip-eq-num-input"
                />
                <span className="clip-eq-unit">Hz</span>
              </div>
            </div>

            {/* Gain (-∞ dB to +6 dB) */}
            <div className="clip-eq-field-row">
              <span className="clip-eq-field-title">Gain (-∞/+6dB):</span>
              <input
                type="range"
                min={SLIDER_MIN_DB}
                max={CLIP_EQ_PEAK_MAX_DB}
                step={0.5}
                value={peakGainDb}
                disabled={!enabled}
                title="Adjust Bell EQ Gain (-∞ dB to +6 dB)"
                onChange={(e) => setPeakGainDb(Number(e.target.value))}
                className="clip-eq-range clip-eq-range--peak"
              />
              <div className="clip-eq-value-display" title="Bell EQ Boost / Cut Gain in Decibels">
                <span className="clip-eq-gain-text">
                  {formatGainDisplay(peakGainDb)}
                </span>
                <span className="clip-eq-unit">dB</span>
              </div>
            </div>

            {/* Bandwidth Q (0.1 to 10.0) */}
            <div className="clip-eq-field-row">
              <span className="clip-eq-field-title">Q Factor:</span>
              <input
                type="range"
                min={0.1}
                max={10.0}
                step={0.1}
                value={peakQ}
                disabled={!enabled}
                title="Adjust Bell EQ Bandwidth / Q Factor (0.1 to 10.0)"
                onChange={(e) => setPeakQ(Number(e.target.value))}
                className="clip-eq-range clip-eq-range--peak"
              />
              <div className="clip-eq-value-display" title="Bell EQ Resonance Quality Factor (Q)">
                <input
                  type="number"
                  min={0.1}
                  max={10.0}
                  step={0.1}
                  value={peakQ}
                  disabled={!enabled}
                  onChange={(e) => setPeakQ(Math.max(0.1, Math.min(10.0, Number(e.target.value))))}
                  className="clip-eq-num-input"
                />
                <span className="clip-eq-unit">Q</span>
              </div>
            </div>
          </div>

          {/* Card 3: LPF Control */}
          <div className={`clip-eq-param-card ${enabled ? "clip-eq-param-card--lpf" : "clip-eq-param-card--disabled"}`}>
            <div className="clip-eq-param-label">
              <span className="clip-eq-indicator clip-eq-indicator--lpf" />
              LOW-PASS (LPF)
            </div>
            <div className="clip-eq-field-row">
              <span className="clip-eq-field-title">Cutoff:</span>
              <input
                type="range"
                min={20}
                max={20000}
                step={10}
                value={lpHz}
                disabled={!enabled}
                title="Adjust Low-Pass Cutoff Frequency (Hz)"
                onChange={(e) => setLpHz(Math.max(Number(e.target.value), hpHz + 50))}
                className="clip-eq-range clip-eq-range--lpf"
              />
              <div className="clip-eq-value-display" title="Low-Pass Cutoff Frequency in Hertz">
                <input
                  type="number"
                  min={20}
                  max={20000}
                  value={lpHz}
                  disabled={!enabled}
                  onChange={(e) => setLpHz(Math.max(Math.min(20000, Number(e.target.value)), hpHz + 50))}
                  className="clip-eq-num-input"
                />
                <span className="clip-eq-unit">Hz</span>
              </div>
            </div>
          </div>
          {/* Card 4: Full Clip Gain Control (-∞ dB to +12 dB) */}
          <div className={`clip-eq-param-card ${enabled ? "clip-eq-param-card--gain" : "clip-eq-param-card--disabled"}`}>
            <div className="clip-eq-param-label">
              <span className="clip-eq-indicator clip-eq-indicator--gain" />
              FULL CLIP GAIN (TRIM / VOLUME)
            </div>
            <div className="clip-eq-field-row">
              <span className="clip-eq-field-title">Gain (-∞/+12dB):</span>
              <input
                type="range"
                min={SLIDER_MIN_DB}
                max={CLIP_EQ_GAIN_MAX_DB}
                step={0.5}
                value={gainDb}
                disabled={!enabled}
                title="Adjust overall clip volume / gain (-∞ dB to +12 dB)"
                onChange={(e) => setGainDb(Number(e.target.value))}
                className="clip-eq-range clip-eq-range--gain"
              />
              {/* La valeur se tape autant qu'elle se glisse. Le curseur avance
                  par demi-décibels et impose de viser; au clavier on écrit
                  −4,5 et c'est réglé.
                  Le brouillon n'est appliqué qu'à la validation : chaque frappe
                  intermédiaire d'un « −12 » passerait sinon par « − », puis
                  « −1 », que le moteur jouerait au vol. */}
              <div className="clip-eq-value-display" title="Overall Clip Trim / Volume Gain in Decibels — type a value or drag">
                <input
                  className="clip-eq-gain-text clip-eq-gain-input"
                  type="text"
                  inputMode="text"
                  disabled={!enabled}
                  aria-label="Clip gain in decibels"
                  value={gainDraft ?? formatGainDisplay(gainDb)}
                  onChange={(event) => setGainDraft(event.target.value)}
                  onFocus={(event) => event.target.select()}
                  onBlur={commitGain}
                  onKeyDown={(event) => {
                    if (event.key === "Enter") {
                      event.preventDefault();
                      commitGain();
                      event.currentTarget.blur();
                    }
                    if (event.key === "Escape") {
                      event.preventDefault();
                      setGainDraft(null);
                      event.currentTarget.blur();
                    }
                  }}
                />
                <span className="clip-eq-unit">dB</span>
              </div>
            </div>
          </div>
        </div>

        {/* Toolbar & Action Footer */}
        <div className="clip-eq-footer">
          <div className="clip-eq-presets">
            <button
              className={`clip-eq-toggle-btn ${enabled ? "clip-eq-toggle-btn--active" : ""}`}
              onClick={() => setEnabled(!enabled)}
              title="Toggle EQ Processing On or Bypass"
            >
              {enabled ? "EQ ENABLED" : "BYPASS"}
            </button>
            <button className="clip-eq-preset-btn" onClick={handleReset} title="Reset EQ parameters to flat defaults">
              RESET (FLAT)
            </button>
          </div>

          <div className="clip-eq-actions">
            <button className="clip-eq-btn clip-eq-btn--save" onClick={onClose} title="Close EQ settings window">
              Close
            </button>
          </div>
        </div>
      </div>
    </div>
  );
};
