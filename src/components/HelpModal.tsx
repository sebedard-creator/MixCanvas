import React, { useEffect } from "react";

import { TransportGlyph } from "./TransportGlyph";

interface HelpModalProps {
  isOpen: boolean;
  onClose: () => void;
  onOpenAbout: () => void;
  /**
   * Vrai quand la fenêtre ABOUT est par-dessus.
   *
   * `Esc` appartient alors à celle du dessus, et à elle seule : sans ce
   * drapeau les deux se fermaient d'une pression. On pourrait s'en remettre à
   * l'ordre des écouteurs — capture avant bulle — mais c'est un raisonnement
   * qui se casse dès qu'on déplace une ligne.
   */
  isCoveredByAbout: boolean;
  /** Faux quand la timeline est déjà vide, ou qu'une édition est en cours. */
  canClearTimeline: boolean;
  onClearTimeline: () => void;
  canClearEverything: boolean;
  onClearEverything: () => void;
}

/**
 * Le classement suit **le geste**, pas le sujet.
 *
 * L'ancien découpage — Transport, Navigation, Editing, Controls & FX — versait
 * dix-sept entrées dans la dernière catégorie : des touches, des glissés et des
 * boutons mélangés, sans qu'on sache où chercher. Or on n'ouvre pas cette
 * fenêtre en se demandant « quelle est la catégorie de ce que je veux faire »,
 * mais « qu'est-ce que je peux appuyer, glisser, cliquer ». Trois familles, et
 * chaque entrée n'appartient qu'à une seule.
 */
type Surface = "key" | "pointer" | "control";

interface ShortcutItem {
  /**
   * Le texte d'une touche, ou le dessin d'un bouton qui n'en porte pas.
   *
   * La chaîne du sidechain était ici un caractère — `⛓` — que le système
   * dessinait à sa façon, sans rapport avec le bouton du clip. Un bouton sans
   * texte doit montrer **sa** marque, prise à la source commune.
   */
  keys: (string | React.ReactNode)[];
  description: string;
  surface: Surface;
  /** Sous-titre d'un petit paquet, à l'intérieur d'une famille. */
  group: string;
}

const SHORTCUTS: ShortcutItem[] = [
  // ── Au clavier ────────────────────────────────────────────────────────────
  { surface: "key", group: "Transport", keys: ["Space"], description: "Play or pause — drives the Beatgrid Editor's preview while it is open" },
  { surface: "key", group: "Transport", keys: ["T"], description: "Zoom in" },
  { surface: "key", group: "Transport", keys: ["R"], description: "Zoom out — keep going to fit the whole project" },
  { surface: "key", group: "Transport", keys: ["Esc"], description: "Close the window on top" },

  { surface: "key", group: "On the selected track", keys: ["B"], description: "Split its clip at the playhead" },
  { surface: "key", group: "On the selected track", keys: ["Delete"], description: "Remove the clip under the pointer — the mouse picks it, not the selection" },
  { surface: "key", group: "On the selected track", keys: ["V"], description: "Add a volume node at the playhead" },
  { surface: "key", group: "On the selected track", keys: ["P"], description: "Add a pan node at the playhead" },
  { surface: "key", group: "On the selected track", keys: ["Shift", "S"], description: "Solo" },
  { surface: "key", group: "On the selected track", keys: ["Shift", "M"], description: "Mute" },

  { surface: "key", group: "Automation lines", keys: ["E"], description: "Cycle what is shown: pan, volume, both, hidden" },
  { surface: "key", group: "Automation lines", keys: ["S"], description: "Cycle the pencil shape — needs volume or pan on screen, not both" },
  { surface: "key", group: "Automation lines", keys: ["D"], description: "Cycle the pencil period — needs volume or pan on screen, not both" },

  { surface: "key", group: "History", keys: ["Ctrl", "Z"], description: "Undo" },
  { surface: "key", group: "History", keys: ["Ctrl", "Y"], description: "Redo" },

  // ── À la souris ───────────────────────────────────────────────────────────
  { surface: "pointer", group: "Moving around", keys: ["Scroll"], description: "Zoom the timeline" },
  { surface: "pointer", group: "Moving around", keys: ["Shift", "Scroll"], description: "Scroll sideways — the view follows the playhead again after a pause" },
  { surface: "pointer", group: "Moving around", keys: ["Click a track"], description: "Select it — anywhere inside counts" },

  // Un seul outil, dont la position décide. L'ordre des lignes est celui des
  // zones, de haut en bas du clip, pour qu'on lise la règle et pas trois cas.
  { surface: "pointer", group: "Clips", keys: ["Drag an edge"], description: "Trim the head or tail — the audio under the rest stays put, and dragging back out restores it" },
  { surface: "pointer", group: "Clips", keys: ["Drag the title bar"], description: "Move along the timeline, or onto another track" },
  { surface: "pointer", group: "Clips", keys: ["Drag the body"], description: "Draw the pencil's shape across the drag, live, onto whichever line VIEW is showing — nothing to arm" },
  { surface: "pointer", group: "Clips", keys: ["Drag from library"], description: "Drop a track straight onto the lane you want" },

  { surface: "pointer", group: "Clips", keys: ["Right click a track or clip"], description: "Shift its downbeat a beat either way, when analysis put the bar line on the 2 or the 3. The tempo does not move, and every clip of that track follows" },
  { surface: "pointer", group: "Automation", keys: ["Right click"], description: "Add a volume or pan node; right-click a node to delete it" },
  { surface: "pointer", group: "Automation", keys: ["Drag a volume node"], description: "Set its level and position — to the bottom of the travel for silence" },
  { surface: "pointer", group: "Automation", keys: ["Drag a pan node"], description: "Up sends the track left, down sends it right" },
  { surface: "pointer", group: "Automation", keys: ["Drag the tempo point"], description: "Ramp the project tempo from a clip's turquoise marker" },
  { surface: "pointer", group: "Automation", keys: ["Right click the ruler"], description: "Type the tempo of the nearest marker — it is that track's BPM, same as the Beatgrid Editor" },

  { surface: "pointer", group: "Filter band", keys: ["Drag"], description: "Draw a filter curve" },
  { surface: "pointer", group: "Filter band", keys: ["Shift", "Drag"], description: "Draw it symmetrical — a triangle" },
  { surface: "pointer", group: "Filter band", keys: ["Ctrl", "Drag"], description: "Draw freehand: the band follows the pointer and closes itself at bypass" },
  { surface: "pointer", group: "Filter band", keys: ["Drag an edge"], description: "Lengthen or shorten a curve, snapped to the grid" },
  { surface: "pointer", group: "Filter band", keys: ["Right click"], description: "Delete the curve under the cursor" },

  // ── Les boutons ───────────────────────────────────────────────────────────
  { surface: "control", group: "On a clip", keys: ["VOX", "MUS"], description: "Play the vocals or the instrumental alone — the first click separates this clip" },
  { surface: "control", group: "On a clip", keys: [<TransportGlyph name="sidechain" />], description: "Make it the sidechain key: it goes silent where it overlaps, and pumps what it covers" },
  { surface: "control", group: "On a clip", keys: ["EQ"], description: "Open its three-band equaliser and gain trim" },
  { surface: "control", group: "On a clip", keys: ["BAKE"], description: "Render its EQ and this lane's automation into a file of its own, then flatten the lane under it — draw freely on top" },
  { surface: "control", group: "On a clip", keys: ["BAKE"], description: "Click a baked clip again to undo it: the automation comes back, replacing anything drawn since" },

  { surface: "control", group: "Transport rail", keys: ["COMP"], description: "Master glue compressor and its console colour" },
  { surface: "control", group: "Transport rail", keys: ["LIMIT"], description: "Master limiter on the output bus" },
  { surface: "control", group: "Transport rail", keys: ["VIEW"], description: "Cycle the automation lines shown" },
  { surface: "control", group: "Transport rail", keys: ["DRAW"], description: "What the pencil draws — shape on the left, period on the right. Off unless VIEW shows volume or pan on its own" },
  { surface: "control", group: "Transport rail", keys: ["AUTO"], description: "Autoplay, along the bottom of PLAY — off, a click in the timeline only moves the playhead" },
  { surface: "control", group: "Transport rail", keys: ["Effect pads"], description: "Hold one while the mix runs — reverb, flange, crush or delay onto that track. Let go and the pass is written onto the timeline" },
  { surface: "control", group: "Transport rail", keys: ["Eraser"], description: "The last pad on each track — sweep it while the music runs to wipe every effect it passes over" },

  { surface: "control", group: "Project", keys: ["BOUNCE MIX"], description: "Render the whole timeline offline to a 16-bit 44.1 kHz stereo WAV, with an optional mastering limiter — a brickwall with look-ahead that lifts the mix to its ceiling and never lets anything past it" },
  { surface: "control", group: "Project", keys: ["SAVE", "LOAD"], description: "Write or reopen a .mixcanvas file — library, beatgrids and timeline together" },
];

const SURFACES: { id: Surface; title: string; hint: string }[] = [
  { id: "key", title: "Keyboard", hint: "No modifier unless one is shown" },
  { id: "pointer", title: "Mouse", hint: "Where you point decides what you get" },
  { id: "control", title: "Buttons", hint: "What the on-screen controls do" },
];

export const HelpModal: React.FC<HelpModalProps> = ({
  isOpen,
  onClose,
  onOpenAbout,
  isCoveredByAbout,
  canClearTimeline,
  onClearTimeline,
  canClearEverything,
  onClearEverything,
}) => {
  useEffect(() => {
    if (!isOpen || isCoveredByAbout) return;

    const handleKeyDown = (event: KeyboardEvent) => {
      if (event.key === "Escape") {
        onClose();
      }
    };

    window.addEventListener("keydown", handleKeyDown);
    return () => window.removeEventListener("keydown", handleKeyDown);
  }, [isOpen, isCoveredByAbout, onClose]);

  if (!isOpen) return null;

  return (
    <div className="help-modal-overlay" onClick={onClose} role="dialog" aria-modal="true" aria-labelledby="help-modal-title">
      <div className="help-modal-content" onClick={(event) => event.stopPropagation()}>
        <div className="help-modal-header">
          <div className="help-modal-title-group">
            <div className="help-modal-badge">
              <span className="help-modal-badge-dot" />
              <span>MixCanvas REFERENCE</span>
            </div>
            <h2 id="help-modal-title">Keyboard Shortcuts &amp; Control Guide</h2>
          </div>
          <button
            className="help-modal-close-btn"
            type="button"
            onClick={onClose}
            title="Close Help Guide (Esc)"
          >
            ✕
          </button>
        </div>

        <div className="help-modal-body">
          {SURFACES.map((surface) => {
            const items = SHORTCUTS.filter((item) => item.surface === surface.id);
            // L'ordre de première apparition fait foi : il est écrit plus haut
            // et se lit dans le fichier, plutôt que d'être trié en douce.
            const groups = items.reduce<string[]>((seen, item) => {
              if (!seen.includes(item.group)) seen.push(item.group);
              return seen;
            }, []);

            return (
              <section className="help-surface" key={surface.id}>
                <header className="help-surface-head">
                  <h3>{surface.title}</h3>
                  <p>{surface.hint}</p>
                </header>

                {groups.map((group) => (
                  <div className="help-group" key={group}>
                    <h4 className="help-group-title">{group}</h4>
                    <div className="help-shortcut-list">
                      {items
                        .filter((item) => item.group === group)
                        .map((item, index) => (
                          <div className="help-shortcut-row" key={index}>
                            <div className="help-keys-group">
                              {item.keys.map((key, keyIndex) => (
                                <React.Fragment key={keyIndex}>
                                  {keyIndex > 0 && (
                                    /* Deux touches pressées ensemble, ou deux
                                       boutons voisins : le signe le dit. */
                                    <span className="help-key-plus">
                                      {item.surface === "control" ? "·" : "+"}
                                    </span>
                                  )}
                                  <kbd
                                    className={`help-keycap${typeof key === "string" ? "" : " help-keycap--glyph"}`}
                                  >
                                    {key}
                                  </kbd>
                                </React.Fragment>
                              ))}
                            </div>
                            <span className="help-shortcut-desc">{item.description}</span>
                          </div>
                        ))}
                    </div>
                  </div>
                ))}
              </section>
            );
          })}
        </div>

        <div className="help-modal-footer">
          <div className="help-footer-secondary">
            <button className="help-about-btn" type="button" onClick={onOpenAbout}>
              ABOUT
            </button>
            {/* Le seul geste destructeur du programme. Il vivait entre PLAY et
                PAUSE, à portée du pouce; ici il faut ouvrir une fenêtre pour
                l'atteindre, ce qui est exactement la friction qu'il mérite. */}
            <button
              className="help-danger-btn"
              type="button"
              disabled={!canClearTimeline}
              onClick={onClearTimeline}
              title={
                canClearTimeline
                  ? "Remove every clip from the timeline — your library is kept"
                  : "The timeline is already empty"
              }
            >
              CLEAR TIMELINE
            </button>
            <button
              className="help-danger-btn help-danger-btn--severe"
              type="button"
              disabled={!canClearEverything}
              onClick={onClearEverything}
              title={
                canClearEverything
                  ? "Start over: empty the timeline and the library — your audio files are never touched"
                  : "There is nothing left to clear"
              }
            >
              CLEAR TIMELINE &amp; LIBRARY
            </button>
          </div>
          <span className="help-modal-tip">
            Press <kbd className="help-keycap help-keycap-sm">Esc</kbd> anytime to exit
          </span>
          <button className="help-modal-done-btn" type="button" onClick={onClose}>
            CLOSE GUIDE
          </button>
        </div>
      </div>
    </div>
  );
};
