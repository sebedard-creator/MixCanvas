import React, { useEffect, useState } from "react";
import { getVersion } from "@tauri-apps/api/app";

interface AboutModalProps {
  isOpen: boolean;
  onClose: () => void;
}

/**
 * L'auteur, tel qu'il veut être nommé.
 *
 * En un seul endroit : la fenêtre le montre, et rien d'autre dans le programme
 * n'a à le connaître.
 */
const AUTHOR = "Sébastien Bédard";
const CONTACT = "sebedard@gmail.com";

interface Credit {
  name: string;
  licence: string;
  role: string;
}

/**
 * Ce que le programme embarque, et sous quelles conditions.
 *
 * Les licences sont relevées des manifestes, pas de mémoire : `cargo metadata`
 * pour la partie Rust, `package.json` pour la partie web. Une liste approximative
 * serait pire que pas de liste — c'est un document légal.
 */
const CREDITS: Credit[] = [
  { name: "Tauri", licence: "MIT / Apache-2.0", role: "Application shell and native bridge" },
  { name: "React", licence: "MIT", role: "Interface" },
  { name: "Vite", licence: "MIT", role: "Build tooling" },
  { name: "rodio", licence: "MIT / Apache-2.0", role: "Audio playback" },
  { name: "cpal", licence: "Apache-2.0", role: "Audio device access" },
  { name: "Symphonia", licence: "MPL-2.0", role: "MP3 and WAV decoding" },
  { name: "LAME 3.100", licence: "LGPL-2.1", role: "MP3 encoding on bounce" },
  { name: "rubato", licence: "MIT", role: "Sample rate conversion" },
  { name: "rusqlite · SQLite", licence: "MIT · Public domain", role: "Library and project storage" },
  { name: "RustFFT", licence: "MIT / Apache-2.0", role: "Fourier transform behind stem separation" },
  { name: "ort", licence: "MIT / Apache-2.0", role: "ONNX Runtime bindings" },
  { name: "ONNX Runtime", licence: "MIT", role: "Neural network inference" },
  { name: "Open-Unmix (UMX-HQ)", licence: "MIT", role: "Vocal separation model" },
  { name: "RTen", licence: "MIT / Apache-2.0", role: "Runs the beat-tracking model" },
  { name: "beat-this-rs", licence: "MIT", role: "Beat and downbeat tracker" },
  { name: "Serde", licence: "MIT / Apache-2.0", role: "Serialisation" },
];

export const AboutModal: React.FC<AboutModalProps> = ({ isOpen, onClose }) => {
  useEffect(() => {
    if (!isOpen) return;

    const handleKeyDown = (event: KeyboardEvent) => {
      if (event.key === "Escape") {
        onClose();
      }
    };

    window.addEventListener("keydown", handleKeyDown);
    return () => window.removeEventListener("keydown", handleKeyDown);
  }, [isOpen, onClose]);

  /* Le numéro vient de l'exécutable lui-même, pas d'une constante recopiée.
     Trois manifestes portent cette version et rien ne garantit qu'ils restent
     d'accord; celui-ci est celui qui tourne. */
  const [version, setVersion] = useState<string | null>(null);
  useEffect(() => {
    if (!isOpen) return;
    let current = true;
    void getVersion()
      .then((value) => {
        if (current) setVersion(value);
      })
      .catch(() => {
        // Sans réponse, la ligne se tait plutôt que d'annoncer un faux numéro.
      });
    return () => {
      current = false;
    };
  }, [isOpen]);

  if (!isOpen) return null;

  return (
    <div
      className="help-modal-overlay about-modal-overlay"
      onClick={onClose}
      role="dialog"
      aria-modal="true"
      aria-labelledby="about-modal-title"
    >
      <div className="help-modal-content about-modal-content" onClick={(event) => event.stopPropagation()}>
        <div className="help-modal-header">
          <div className="help-modal-title-group">
            <div className="help-modal-badge">
              <span className="help-modal-badge-dot" />
              <span>ABOUT</span>
            </div>
            <h2 id="about-modal-title">
              MixCanvas
              {version && <span className="about-version">v{version}</span>}
            </h2>
          </div>
          <button className="help-modal-close-btn" type="button" onClick={onClose} title="Close (Esc)">
            ✕
          </button>
        </div>

        <div className="help-modal-body about-modal-body">
          <section className="about-block">
            <h3>A timeline-based DJ mix editor</h3>
            <p className="about-author">{AUTHOR}</p>
            <p className="about-contact">{CONTACT}</p>
          </section>

          <section className="about-block">
            <h3>Licence</h3>
            <p>
              MixCanvas is free software released under the <strong>GNU Affero General
              Public License, version 3</strong>. You may use, study, share and modify it.
              If you run a modified version and let others use it — including over a
              network — you must offer them its source code under the same licence.
            </p>
            <p className="about-fineprint">
              This program comes with absolutely no warranty. The full text ships with
              the program as the LICENSE file.
            </p>
            {/* La licence exige déjà de conserver les mentions et de signaler ce
                qu'on a changé. Écrire « une attribution serait appréciée » sans
                le dire ferait passer une obligation pour une faveur, et
                l'auteur y perdrait ce que la licence lui accorde. La demande
                humaine vient donc après, et se donne pour ce qu'elle est. */}
            <p>
              Forks are welcome. The licence already asks you to keep the copyright
              notices and to state what you changed. Beyond that — and as a courtesy
              rather than a condition — naming MixCanvas and linking back to the
              original project would be genuinely appreciated.
            </p>
          </section>

          <section className="about-block">
            <h3>Built with</h3>
            <ul className="about-credits">
              {CREDITS.map((credit) => (
                <li key={credit.name}>
                  <span className="about-credit-name">{credit.name}</span>
                  <span className="about-credit-role">{credit.role}</span>
                  <span className="about-credit-licence">{credit.licence}</span>
                </li>
              ))}
            </ul>
            <p className="about-fineprint">
              Symphonia is covered by the Mozilla Public License 2.0: its source, and any
              change made to it, must remain available. MixCanvas uses it unmodified.
            </p>
          </section>

          {/* La détection de tempo est ce qu'on interroge en premier quand une
              grille tombe à côté. Dire par quoi elle passe évite d'avoir à lire
              le code pour savoir si le résultat vient d'un modèle ou d'un
              repli, et où corriger à la main quand il se trompe. */}
          <section className="about-block">
            <h3>How the beat grid is found</h3>
            <p>
              Tempo and downbeats come from <strong>Beat-This</strong>, a neural beat
              tracker, run locally through RTen. Its beat events are then fitted to a
              single rigid grid — a DJ needs a constant clock, not a list of musical
              events — and the first downbeat is placed where the kick actually enters,
              so an ambient intro does not push the grid off.
            </p>
            <p>
              When the model cannot run or the fitted tempo falls outside the supported
              range, the older correlation-based analyser takes over. Either way the
              result is only a starting point: the <strong>Beatgrid Editor</strong> lets
              you tap, nudge and correct it, and can restore the detected values.
            </p>
            <p className="about-fineprint">
              Beat This! is the work of the Institute of Computational Perception at JKU
              Linz, with the Rust port by danigb. The models ship unmodified under the
              MIT licence; the full notice is in THIRD_PARTY_NOTICES.
            </p>
          </section>

          {/* Un drapeau de lancement ne se découvre pas tout seul. Celui-ci a
              longtemps été présenté comme un dépannage d'affichage; il est en
              réalité un choix mesuré, et le dire ainsi évite qu'on l'active en
              croyant gagner quelque chose. */}
          <section className="about-block">
            <h3>Why the interface draws in software</h3>
            <p>
              MixCanvas draws its interface in <strong>software</strong> by default, and that is a
              deliberate choice rather than a fallback. On the machines we profiled, turning
              hardware acceleration on made no measurable difference to how the timeline performs —
              while some graphics drivers make WebView2's hardware compositor tear during a zoom.
              Given a choice between no gain and a possible artefact, correctness wins.
            </p>
            <p>
              Nothing about your audio depends on this. Playback, analysis, mixing and bouncing are
              native Rust and never touch the browser's renderer.
            </p>
            <p className="about-fineprint">
              You can still turn it on. Launch with <strong>--gpu-safe</strong> to draw on the
              graphics card while compositing in software, or <strong>--gpu</strong> for full
              hardware acceleration. <strong>--no-gpu</strong> names the default explicitly. The
              last flag on the line wins, and the portable build ships a{" "}
              <strong>.cmd</strong> shortcut for each. If your machine turns out to gain from it,
              the flag is all you need — no rebuild.
            </p>
          </section>

          <section className="about-block">
            <h3>Your music</h3>
            <p>
              Nothing leaves this machine. Tracks are read where they sit, and the program
              makes no network request of any kind.
            </p>
            {/* On finit toujours par se demander où sont passés ces gigaoctets.
                Le dire ici évite d'avoir à le chercher. */}
            <p>
              <strong>Everything MixCanvas writes lives in one folder beside the program</strong>,
              called <strong>MixCanvas Files</strong>: the library database, the models unpacked
              from the executable, and the WAV files for separated stems and baked clips. Copy
              the program and that folder together and you have moved your whole setup; delete
              the folder and you are back to a fresh install. Nothing is hidden away.
            </p>
            <p>
              Each project gets a folder of its own, named after it; an unsaved session lives in{" "}
              <strong>Scratch</strong> until you name it, and its media follow when you do.
            </p>
            <p className="about-fineprint">
              The one exception: a program placed somewhere it may not write — Program Files, a
              read-only share — falls back to the application data folder, because refusing to
              start would be worse.
            </p>
            <p className="about-fineprint">
              On exit, files that nothing refers to any more are deleted. A file that is
              still referenced is never touched, whether or not the current session uses
              it — a separation costs minutes, and the safe test is the strict one.
            </p>
          </section>
        </div>

        <div className="help-modal-footer">
          <button className="help-modal-done-btn" type="button" onClick={onClose}>
            CLOSE
          </button>
        </div>
      </div>
    </div>
  );
};
