import React, { useEffect } from "react";

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
  { name: "rusqlite · SQLite", licence: "MIT · Public domain", role: "Library and project storage" },
  { name: "RustFFT", licence: "MIT / Apache-2.0", role: "Fourier transform behind stem separation" },
  { name: "ort", licence: "MIT / Apache-2.0", role: "ONNX Runtime bindings" },
  { name: "ONNX Runtime", licence: "MIT", role: "Neural network inference" },
  { name: "Open-Unmix (UMX-HQ)", licence: "MIT", role: "Vocal separation model" },
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
            <h2 id="about-modal-title">MixCanvas</h2>
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

          <section className="about-block">
            <h3>Your music</h3>
            <p>
              Nothing leaves this machine. Tracks are read where they sit, analysis and
              separated stems are cached beside the library, and the program makes no
              network request of any kind.
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
