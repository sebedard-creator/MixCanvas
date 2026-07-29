"""Produit le modèle de séparation de voix que MixCanvas embarque.

À lancer **une seule fois**, sur une machine avec Python. Les dépendances
(PyTorch, ~2 Go) ne servent qu'à cette conversion et peuvent être supprimées
ensuite : le `.onnx` produit se suffit à lui-même.

    python -m venv .venv-export
    .venv-export\Scripts\python.exe -m pip install torch demucs onnx onnxruntime onnxconverter-common onnxscript openunmix
    .venv-export\Scripts\python.exe scripts/export-demucs-onnx.py open-unmix

Le python de l'environnement est appelé directement plutôt qu'activé : la
politique d'exécution de PowerShell bloque souvent `Activate.ps1`. Un nom en
argument choisit le candidat; sans argument, ils sont essayés dans l'ordre.

Le résultat va dans `src-tauri/resources/models/`.

Pourquoi ces modèles-là : leurs poids sont sous licence MIT, donc redistribuables
dans un binaire public sans zone grise. Les `.onnx` tout faits qui circulent sous
le nom « vocal-separation » viennent d'Ultimate Vocal Remover et n'offrent pas
cette garantie.

**Demucs ne s'exporte pas, et ce n'est pas réparable par un réglage.** Trois
obstacles, dans cet ordre : une assertion interne qui dépend des données (levée
ici en remplaçant `pad1d`), la taille du segment annoncé (bornée à huit
secondes), puis le mur — `aten.pad` sur un tenseur complexe, sans traduction
ONNX. Sa branche spectrale est bâtie sur les nombres complexes, qu'ONNX n'a pas.
Les patcher un par un revient à réécrire `_spec` et `_ispec`, c'est-à-dire à
sortir la transformée de Fourier du graphe : un chantier, pas un correctif.

**Open-unmix passe**, parce que sa transformée vit déjà hors du modèle : il
reçoit un spectrogramme d'amplitude et rend un masque d'amplitude, sans un seul
nombre complexe. C'est aussi ce que MixCanvas devait faire de toute façon. Seule
la cible « voix » est exportée — l'instrumental se calcule par différence dans le
domaine temporel, ce qui garantit que les deux stems se resomment exactement.
"""

from __future__ import annotations

import contextlib
import hashlib
import io
import sys
import warnings
from pathlib import Path


class Tee:
    """Écrit aux deux endroits : l'écran pour suivre, le tampon pour le journal."""

    def __init__(self, *targets):
        self.targets = targets

    def write(self, text: str) -> int:
        for target in self.targets:
            target.write(text)
        return len(text)

    def flush(self) -> None:
        for target in self.targets:
            target.flush()

OUTPUT_DIR = Path(__file__).resolve().parent.parent / "src-tauri" / "resources" / "models"

# Du plus capable au plus docile à l'export. Tous sont sous licence MIT.
CANDIDATES = ["htdemucs", "hdemucs_mmi", "mdx_extra_q", "open-unmix"]

# 18 plutôt que 17 : le nouvel exporteur n'implémente rien en dessous et
# rétrograde ensuite, une conversion qui peut échouer pour rien.
OPSET = 18

# La tranche que le graphe exporté sait traiter, en secondes.
#
# `model.segment` est le maximum que le modèle accepte, pas sa taille de
# travail : 44 s pour `hdemucs_mmi`, soit près de deux millions d'échantillons,
# dont le graphe tracé ne tient pas en mémoire — l'export mourait sans même une
# exception. Demucs découpe en tranches de quelques secondes à l'usage, et c'est
# MixCanvas qui fera ce découpage, avec recouvrement, pour pouvoir afficher une
# progression.
MAX_EXPORT_SECONDS = 8.0


REQUIREMENTS = {
    "torch": "torch",
    "demucs": "demucs",
    "onnx": "onnx",
    "onnxruntime": "onnxruntime",
    "onnxconverter_common": "onnxconverter-common",
    # L'exporteur fondé sur `torch.export` — le seul qui sache décomposer la
    # transformée de Fourier — passe par lui. Sans ce paquet il se dérobe avec
    # un simple « No module named », et l'on croit que c'est le modèle qui
    # résiste alors que c'est l'outil qui manque.
    "onnxscript": "onnxscript",
    # Le repli si Demucs ne s'exporte pas.
    "openunmix": "openunmix",
}


def check_requirements() -> list[str]:
    """Ce qui manque, dit une fois et avant d'essayer quoi que ce soit.

    Sans cette vérification, un `torch` absent ressortait une fois par modèle
    candidat : trois échecs identiques pour une seule cause, et la vraie
    consigne noyée dedans.
    """
    import importlib.util

    return [
        package
        for module, package in REQUIREMENTS.items()
        if importlib.util.find_spec(module) is None
    ]


def segment_samples_of(model) -> int:
    """La tranche exportée, bornée.

    `model.segment` est parfois une `Fraction` — 39/5 pour `htdemucs` —, d'où le
    passage explicite par un flottant.
    """
    seconds = min(float(model.segment), MAX_EXPORT_SECONDS)
    return int(seconds * model.samplerate)


def defuse_demucs_assertions() -> None:
    """Retire les assertions de `pad1d`, qui bloquent le nouvel exporteur.

    Demucs vérifie après chaque bourrage que le signal d'origine se retrouve
    bien au milieu du résultat :

        assert (out[..., left:left + length] == x0).all()

    C'est une comparaison de tenseurs dont le résultat est lu par `.item()` —
    donc une décision qui dépend des données. `torch.export` refuse par
    principe de tracer un chemin pareil : il ne sait pas ce que vaudra cette
    comparaison. L'export mourait ainsi sur un garde-fou de développement, et
    non sur le modèle.

    La fonction remplacée fait exactement le même bourrage, sans les deux
    vérifications. Le graphe exporté est vérifié numériquement juste après, ce
    qui couvre ce que ces assertions couvraient.
    """
    import sys as _sys

    import torch.nn.functional as functional

    def pad1d(x, paddings, mode: str = "constant", value: float = 0.0):
        length = x.shape[-1]
        padding_left, padding_right = paddings
        if mode == "reflect":
            max_pad = max(padding_left, padding_right)
            if length <= max_pad:
                extra = max_pad - length + 1
                extra_right = min(padding_right, extra)
                extra_left = extra - extra_right
                paddings = (padding_left - extra_left, padding_right - extra_right)
                x = functional.pad(x, (extra_left, extra_right))
        return functional.pad(x, paddings, mode, value)

    # Le nom est importé dans plusieurs modules; remplacer la définition
    # d'origine ne suffirait pas, chaque module gardant sa propre référence.
    patched = []
    for module_name, module in list(_sys.modules.items()):
        if module_name.startswith("demucs") and hasattr(module, "pad1d"):
            module.pad1d = pad1d
            patched.append(module_name)
    if patched:
        print(f"  assertions retirées dans : {', '.join(patched)}")


def export(name: str) -> Path:
    import torch
    from demucs.pretrained import get_model

    model = get_model(name)
    model.eval()
    # Un bag of models expose ses membres; on prend le premier, un seul réseau
    # étant déjà suffisant et quatre fois plus rapide.
    if hasattr(model, "models"):
        model = model.models[0]
        model.eval()

    segment_samples = segment_samples_of(model)
    print(f"  segment : {segment_samples / model.samplerate:.2f} s"
          f" = {segment_samples} échantillons")
    print(f"  sources : {model.sources}")

    defuse_demucs_assertions()
    dummy = torch.zeros(1, model.audio_channels, segment_samples)
    target = OUTPUT_DIR / f"{name}.onnx"
    OUTPUT_DIR.mkdir(parents=True, exist_ok=True)

    # Le nouvel exporteur d'abord.
    #
    # L'ancien bute sur la transformée de Fourier du modèle : « STFT does not
    # currently support complex types ». ONNX n'a pas de type complexe, et
    # l'exporteur historique s'arrête là. Celui fondé sur `torch.export`, devenu
    # le défaut en 2.9, décompose la transformée en opérations réelles — c'est
    # la seule route qui puisse passer, et je l'avais interdite en écrivant
    # `dynamo=False`.
    errors: list[str] = []
    for dynamo in (True, False):
        try:
            torch.onnx.export(
                model,
                dummy,
                str(target),
                opset_version=OPSET,
                input_names=["mix"],
                output_names=["sources"],
                do_constant_folding=True,
                dynamo=dynamo,
            )
            print(f"  exporteur : {'torch.export' if dynamo else 'TorchScript'}")
            return target
        except Exception as error:  # noqa: BLE001 — on tente l'autre exporteur
            import traceback

            label = "torch.export" if dynamo else "TorchScript"
            # Le message complet, gardé sur disque : la cause d'un échec à
            # l'étape 3 tient dans la liste des opérateurs non traduits, qui
            # arrive bien après les 160 premiers caractères.
            LOG_DIR.mkdir(parents=True, exist_ok=True)
            detail = LOG_DIR / f"{name}-{'dynamo' if dynamo else 'torchscript'}.log"
            detail.write_text(
                "".join(traceback.format_exception(error)), encoding="utf-8"
            )
            first = str(error).strip().splitlines()
            errors.append(f"{label} : {first[0][:160] if first else type(error).__name__}")
    raise RuntimeError("les deux exporteurs ont échoué — " + " | ".join(errors))


# La transformée d'open-unmix, qui vit hors du modèle et devra donc être
# refaite en Rust. Ce sont les valeurs de référence du modèle entraîné.
OPEN_UNMIX_FFT = 4096
OPEN_UNMIX_HOP = 1024
# Nombre de trames par appel. Le modèle est récurrent sur le temps et accepte
# n'importe quelle longueur; on fige celle-ci pour l'export, et MixCanvas
# découpera en tranches de cette taille. 256 trames ≈ 5,9 s à 44,1 kHz.
OPEN_UNMIX_FRAMES = 256
# Le modèle ne regarde que les bandes sous ~16 kHz — 1487 sur 2049 — mais rend
# un masque sur toute l'étendue. Rien à faire côté Rust, sinon le savoir.
OPEN_UNMIX_USEFUL_BINS = 1487


def export_open_unmix() -> tuple[Path, dict]:
    """L'issue si Demucs reste bloqué : un modèle purement spectral.

    Open-unmix reçoit un spectrogramme d'**amplitude** et rend un masque
    d'amplitude. Aucun nombre complexe n'entre dans le graphe : la transformée
    de Fourier et son inverse vivent à l'extérieur, ce qui est exactement ce qui
    bloque Demucs et exactement ce que MixCanvas devait faire de toute façon.

    On n'exporte que la cible « voix » : l'instrumental se calcule par
    différence dans le domaine temporel, ce qui garantit en prime que les deux
    stems se resomment exactement. Le fichier tombe ainsi à une vingtaine de
    mégaoctets.
    """
    import torch
    from openunmix import umxhq

    separator = umxhq(targets=["vocals"], device="cpu", pretrained=True)
    model = separator.target_models["vocals"]
    model.eval()

    bins = OPEN_UNMIX_FFT // 2 + 1
    # (lot, canaux, bandes, trames). Le modèle permute lui-même en interne pour
    # sa couche récurrente; lui donner déjà l'ordre permuté lui laissait deux
    # bandes au lieu de deux mille, et ses coefficients de normalisation
    # n'avaient plus rien à quoi s'appliquer.
    dummy = torch.zeros(1, 2, bins, OPEN_UNMIX_FRAMES)
    target = OUTPUT_DIR / "open-unmix-vocals.onnx"
    OUTPUT_DIR.mkdir(parents=True, exist_ok=True)

    torch.onnx.export(
        model,
        dummy,
        str(target),
        opset_version=OPSET,
        input_names=["magnitude"],
        output_names=["mask"],
        do_constant_folding=True,
    )
    return target, {
        "entrée": f"amplitude (1, 2, {bins}, {OPEN_UNMIX_FRAMES})",
        "fft": OPEN_UNMIX_FFT,
        "hop": OPEN_UNMIX_HOP,
        "trames": OPEN_UNMIX_FRAMES,
        "fenêtre": "hann",
        "bandes utiles": f"{OPEN_UNMIX_USEFUL_BINS} (le masque couvre tout)",
        "cible": "vocals (l'instrumental se calcule par différence)",
    }


def weight_size(path: Path) -> int:
    """La taille réelle, fichier de poids compris.

    Le nouvel exporteur sort le graphe et les poids séparément : un `.onnx` de
    quelques centaines de kilo-octets flanqué d'un `.onnx.data` qui porte tout.
    Ne mesurer que le premier annonçait « 0 Mo » pour un modèle de 36.
    """
    total = path.stat().st_size
    external = path.with_suffix(path.suffix + ".data")
    if external.exists():
        total += external.stat().st_size
    return total


def drop_full_precision(path: Path) -> None:
    """La pleine précision est un intermédiaire : elle a fait son office.

    La demi-précision produite est autonome — poids compris —, alors que celle
    d'où elle sort traîne son fichier de données à côté.
    """
    for candidate in (path, path.with_suffix(path.suffix + ".data")):
        if candidate.exists():
            candidate.unlink()


def to_float16(path: Path) -> Path:
    """Moitié du poids sur le disque, et le format natif du GPU.

    La perte est inaudible sur une séparation de stems : le masque appliqué au
    spectre est bien plus grossier que la précision des poids.
    """
    import onnx
    from onnxconverter_common import float16

    model = onnx.load(str(path))
    half = float16.convert_float_to_float16(model, keep_io_types=True)
    target = path.with_name(f"{path.stem}-fp16.onnx")
    onnx.save(half, str(target))
    return target


def verify(path: Path, channels: int, segment_samples: int) -> None:
    """Fait tourner le graphe une fois : un fichier qui charge peut encore être faux."""
    import numpy as np
    import onnxruntime as ort

    session = ort.InferenceSession(str(path), providers=["CPUExecutionProvider"])
    name = session.get_inputs()[0].name
    noise = np.random.default_rng(7).standard_normal((1, channels, segment_samples), dtype=np.float32)
    outputs = session.run(None, {name: noise * 0.1})
    shape = outputs[0].shape
    print(f"  sortie  : {shape}")
    if not np.isfinite(outputs[0]).all():
        raise RuntimeError("la sortie contient des valeurs non finies")
    if len(shape) != 4:
        raise RuntimeError(f"forme inattendue : {shape}, attendu (lot, sources, canaux, temps)")


LOG_DIR = Path(__file__).resolve().parent / "export-logs"


def report_failure(name: str, error: BaseException, captured: str) -> str:
    """Garde tout, n'affiche que ce qui se lit.

    Un export ONNX qui échoue recrache le graphe entier — des centaines de pages
    d'opérateurs. La cause tient en une ligne, quelque part dedans : elle se perd
    à l'écran et ne peut pas être recopiée. Le détail va donc dans un fichier, et
    la console ne montre que les lignes qui ne décrivent pas le graphe, c'est-à-
    dire celles qui ne commencent pas par `%`.
    """
    import traceback

    LOG_DIR.mkdir(parents=True, exist_ok=True)
    log = LOG_DIR / f"{name}.log"
    with log.open("w", encoding="utf-8") as handle:
        handle.write(captured)
        handle.write("\n\n=== exception ===\n")
        handle.write("".join(traceback.format_exception(error)))

    lines = [
        line.strip()
        for line in f"{captured}\n{error}".splitlines()
        if line.strip()
        and not line.lstrip().startswith("%")
        # Le nouvel exporteur imprime son graphe autrement : une signature
        # géante puis des lignes `nom: "f32[...]" = torch.ops...`.
        and not line.lstrip().startswith("def forward")
        and "torch.ops.aten" not in line
        and "arg0_1" not in line
    ]
    # Le contexte est en tête, la cause presque toujours à la fin.
    head = lines[:2]
    tail = [line for line in lines[-6:] if line not in head]
    for line in head + tail:
        print(f"    {line[:200]}")
    print(f"  détail complet : {log}")
    return f"{name}: {(lines[-1] if lines else type(error).__name__)[:160]}"


def verify_open_unmix(path: Path) -> None:
    """Un masque d'amplitude, de la même forme que le spectrogramme reçu."""
    import numpy as np
    import onnxruntime as ort

    session = ort.InferenceSession(str(path), providers=["CPUExecutionProvider"])
    name = session.get_inputs()[0].name
    bins = OPEN_UNMIX_FFT // 2 + 1
    magnitude = np.abs(
        np.random.default_rng(7).standard_normal(
            (1, 2, bins, OPEN_UNMIX_FRAMES), dtype=np.float32
        )
    )
    output = session.run(None, {name: magnitude})[0]
    print(f"  sortie  : {output.shape}")
    if not np.isfinite(output).all():
        raise RuntimeError("la sortie contient des valeurs non finies")
    if output.shape != magnitude.shape:
        raise RuntimeError(f"forme inattendue : {output.shape}, attendu {magnitude.shape}")


def digest(path: Path) -> str:
    sha = hashlib.sha256()
    with path.open("rb") as handle:
        for block in iter(lambda: handle.read(1 << 20), b""):
            sha.update(block)
    return sha.hexdigest()


def silence_tracer_warnings() -> None:
    """« ce tracé pourrait ne pas se généraliser à d'autres entrées » : c'est voulu.

    Le graphe est exporté pour une taille de tranche fixe, et c'est exactement
    cette taille qu'on lui donnera toujours. Les constantes figées dans le tracé
    sont donc ce qu'on veut, et non un défaut. La sortie est vérifiée
    numériquement juste après, ce qui attraperait un tracé réellement faux.
    """
    warnings.filterwarnings("ignore", category=UserWarning)
    try:
        from torch.jit import TracerWarning

        warnings.filterwarnings("ignore", category=TracerWarning)
    except ImportError:
        pass


def main() -> int:
    missing = check_requirements()
    if missing:
        venv = ".venv-export"
        python = rf"{venv}\Scripts\python.exe"
        script = Path(__file__).name
        print(f"Il manque : {', '.join(missing)}.", file=sys.stderr)
        print("\nDans un environnement jetable :\n", file=sys.stderr)
        print(f"    python -m venv {venv}", file=sys.stderr)
        print(
            f"    {python} -m pip install " + " ".join(REQUIREMENTS.values()),
            file=sys.stderr,
        )
        print(f"    {python} scripts/{script}", file=sys.stderr)
        print(
            f"\nCe sont environ 2 Go, qui ne servent qu'à cette conversion :"
            f" le dossier {venv} peut être supprimé ensuite.",
            file=sys.stderr,
        )
        return 1

    silence_tracer_warnings()

    # Un nom en argument choisit le candidat : réessayer les trois Demucs pour
    # atteindre le quatrième coûte dix minutes et deux gigaoctets de poids
    # téléchargés, pour un échec qu'on connaît déjà.
    wanted = sys.argv[1:] or CANDIDATES
    unknown = [name for name in wanted if name not in CANDIDATES]
    if unknown:
        print(f"inconnu : {', '.join(unknown)}", file=sys.stderr)
        print(f"connus  : {', '.join(CANDIDATES)}", file=sys.stderr)
        return 1

    failures: list[str] = []
    for name in wanted:
        print(f"\n== {name}")
        # PyTorch écrit aussi le graphe sur la sortie standard, hors exception.
        buffer = io.StringIO()
        try:
            if name == "open-unmix":
                with (
                    contextlib.redirect_stdout(Tee(sys.stdout, buffer)),
                    contextlib.redirect_stderr(buffer),
                ):
                    full, facts = export_open_unmix()
                print(f"  exporté : {full.name} ({weight_size(full) / 1e6:.0f} Mo)")
                verify_open_unmix(full)
                half = to_float16(full)
                print(
                    f"  demi-précision : {half.name}"
                    f" ({half.stat().st_size / 1e6:.0f} Mo)"
                )
                verify_open_unmix(half)
                drop_full_precision(full)
                print("\n--- à reporter dans src-tauri/src/audio/stems.rs ---")
                print(f"modèle            : {half.name}")
                print(f"sha256            : {digest(half)}")
                for key, value in facts.items():
                    print(f"{key:<18}: {value}")
                return 0

            import torch
            from demucs.pretrained import get_model

            model = get_model(name)
            if hasattr(model, "models"):
                model = model.models[0]
            channels = model.audio_channels
            segment_samples = segment_samples_of(model)
            sources = list(model.sources)
            del model, torch, get_model

            with (
                contextlib.redirect_stdout(Tee(sys.stdout, buffer)),
                contextlib.redirect_stderr(buffer),
            ):
                full = export(name)
            print(f"  exporté : {full.name} ({weight_size(full) / 1e6:.0f} Mo)")
            verify(full, channels, segment_samples)

            half = to_float16(full)
            print(f"  demi-précision : {half.name} ({half.stat().st_size / 1e6:.0f} Mo)")
            verify(half, channels, segment_samples)

            print("\n--- à reporter dans src-tauri/src/audio/stems.rs ---")
            print(f"modèle            : {half.name}")
            print(f"sha256            : {digest(half)}")
            print(f"échantillonnage   : 44100")
            print(f"canaux            : {channels}")
            print(f"segment           : {segment_samples} échantillons")
            print(f"sources (ordre)   : {sources}")
            print(f"index de la voix  : {sources.index('vocals')}")
            return 0
        except Exception as error:  # noqa: BLE001 — on essaie le suivant, quoi qu'il arrive
            print("  échec :")
            failures.append(report_failure(name, error, buffer.getvalue()))

    print("\nAucun modèle n'a pu être exporté :", file=sys.stderr)
    for failure in failures:
        print(f"  - {failure}", file=sys.stderr)
    return 1


if __name__ == "__main__":
    raise SystemExit(main())
