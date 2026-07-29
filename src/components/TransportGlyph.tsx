/**
 * The marks printed on the transport keycaps.
 *
 * These were emoji — ▶ Ⅱ 🎚 ⚡ — which the operating system renders in its own
 * font, two of them in full colour. No amount of styling could bring them into
 * line with the rest of the interface, because they were never ours to style.
 * Drawn as geometry they take the ink colour of the cap and hold their weight
 * at any scale.
 *
 * COMP and LIMIT share one idea: the transfer curve every dynamics processor is
 * described by. The compressor bends where it starts working; the limiter meets
 * a ceiling and goes flat along it. Read side by side, the pair says what the
 * two controls actually do to a signal.
 */

export type TransportGlyphName =
  | "play"
  | "pause"
  | "comp"
  | "limit"
  | "busy"
  | "bounce"
  | "view-both"
  | "view-volume"
  | "view-pan"
  | "view-none"
  | "draw-off"
  | "draw-step"
  | "draw-sine"
  | "draw-triangle"
  | "sidechain"
  | "autoplay";

const STROKE = {
  fill: "none",
  stroke: "currentColor",
  strokeWidth: 1.4,
  strokeLinecap: "round",
  strokeLinejoin: "round",
} as const;

/**
 * Les deux lignes d'automation, telles qu'elles apparaissent sur une piste :
 * le volume plein et coudé, le panoramique pointillé. La marque ne dit pas
 * « voir » dans l'abstrait, elle montre ce qui sera affiché — et l'état éteint
 * se lit comme les lignes elles-mêmes, effacées.
 */
const VIEW_VOLUME_PATH = "M1.6 6.6 4.8 3.6 10.4 5.2";
const VIEW_PAN_PATH = "M1.6 9.2H10.4";
const VIEW_DIM = 0.2;

function ViewGlyph({ volume, pan }: { volume: boolean; pan: boolean }) {
  return (
    <>
      <path {...STROKE} d={VIEW_VOLUME_PATH} opacity={volume ? 1 : VIEW_DIM} />
      <path
        {...STROKE}
        d={VIEW_PAN_PATH}
        strokeDasharray="2.6 2"
        opacity={pan ? 1 : VIEW_DIM}
      />
    </>
  );
}

export function TransportGlyph({ name }: { name: TransportGlyphName }) {
  return (
    <svg className="transport-glyph" viewBox="0 0 12 12" aria-hidden="true" focusable="false">
      {name === "play" && <path d="M3.6 2.4 9.4 6 3.6 9.6Z" fill="currentColor" />}
      {name === "pause" && (
        <>
          <rect x="3.4" y="2.6" width="1.9" height="6.8" rx="0.4" fill="currentColor" />
          <rect x="6.7" y="2.6" width="1.9" height="6.8" rx="0.4" fill="currentColor" />
        </>
      )}
      {name === "comp" && <path {...STROKE} d="M1.6 10.4 5.4 6.6 10.4 3.4" />}
      {name === "view-both" && <ViewGlyph volume pan />}
      {name === "view-volume" && <ViewGlyph volume pan={false} />}
      {name === "view-pan" && <ViewGlyph volume={false} pan />}
      {name === "view-none" && <ViewGlyph volume={false} pan={false} />}
      {/* Les trois formes du crayon, dessinées comme elles sortiront sur la
          piste. Éteint, c'est la ligne plate : l'absence d'automation. */}
      {name === "draw-off" && <path {...STROKE} d="M1.5 6H10.5" opacity={0.3} />}
      {name === "draw-step" && (
        <path {...STROKE} d="M1.5 8.6H3.6V3.4H6.4V8.6H9.2V3.4H10.5" />
      )}
      {name === "draw-sine" && (
        <path {...STROKE} d="M1.5 6C2.4 2.6 3.6 2.6 4.5 6S6.6 9.4 7.5 6 9.6 2.6 10.5 6" />
      )}
      {name === "draw-triangle" && (
        <path {...STROKE} d="M1.5 8.6 3.75 3.4 6 8.6 8.25 3.4 10.5 8.6" />
      )}
      {name === "bounce" && (
        <>
          {/* Le mix descend vers un support : une flèche vers le bas posée sur
              une ligne. La même géométrie que les autres marques — trait de
              1,4, bouts arrondis — pour que la rangée reste d'une seule main. */}
          <path {...STROKE} d="M6 1.7V7.3" />
          <path {...STROKE} d="M3.5 5 6 7.5 8.5 5" />
          <path {...STROKE} d="M2 10.3H10" />
        </>
      )}
      {name === "limit" && (
        <>
          <path {...STROKE} d="M1.6 10.4 5 4.2H10.4" />
          {/* The ceiling the curve flattens against. */}
          <path {...STROKE} strokeWidth={1} opacity={0.45} d="M1.4 2.2H10.6" />
        </>
      )}
      {name === "autoplay" && (
        <>
          {/* La tête de lecture qu'on vient de poser, et ce qui s'ensuit : le
              trait d'abord, la lecture juste après. La marque raconte le geste
              — un clic dans la timeline — plutôt que le mot « auto ». */}
          <path {...STROKE} d="M2.6 2.2V9.8" />
          <path d="M5.2 3.4 9.6 6 5.2 8.6Z" fill="currentColor" />
        </>
      )}
      {name === "sidechain" && (
        <>
          {/* Deux maillons pris l'un dans l'autre : c'est une chaîne latérale,
              et une clé de serrurier demandait de connaître le mot avant de
              lire le dessin. Trait plus fin que les autres marques — 1,1 au
              lieu de 1,4 : les courbes sont serrées, et à pleine épaisseur les
              deux boucles se referment sur elles-mêmes. */}
          <path
            {...STROKE}
            strokeWidth={1.1}
            d="M5.1 6.9a2.3 2.3 0 0 0 3.45.25l1.35-1.35a2.3 2.3 0 0 0-3.25-3.25l-.8.75"
          />
          <path
            {...STROKE}
            strokeWidth={1.1}
            d="M6.9 5.1a2.3 2.3 0 0 0-3.45-.25L2.1 6.2a2.3 2.3 0 0 0 3.25 3.25l.75-.75"
          />
        </>
      )}
      {name === "busy" && (
        <>
          <circle cx="2.4" cy="6" r="1.05" fill="currentColor" opacity="0.35" />
          <circle cx="6" cy="6" r="1.05" fill="currentColor" opacity="0.65" />
          <circle cx="9.6" cy="6" r="1.05" fill="currentColor" />
        </>
      )}
    </svg>
  );
}
