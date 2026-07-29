/**
 * Un seul outil, dont la position décide.
 *
 * Avant, le crayon s'armait par un bouton et prenait toute la voie : il fallait
 * se rappeler dans quel mode on était, et un mode qu'on ne voit pas se
 * découvre en cassant quelque chose. Ici c'est l'endroit du **premier appui**
 * qui choisit le geste — les bords rognent, la barre du haut déplace, le corps
 * dessine, et le vide de la voie place la tête de lecture.
 *
 * Le geste choisi ne change plus ensuite : la capture du pointeur le garde
 * jusqu'au relâchement, de sorte qu'un trait commencé dans le corps continue de
 * dessiner même s'il passe sur la barre.
 */

import { type TrimmableClip, trimEdgeAtPointer } from "./clipTrim";

export type SmartTool = "trim-start" | "trim-end" | "move" | "draw";

export interface SmartToolContext {
  /** Le temps sous le pointeur, en coordonnées de timeline. */
  beat: number;
  /** La hauteur du pointeur dans le clip, en pixels depuis son haut. */
  offsetY: number;
  /** La hauteur de la barre de titre, qui porte le nom et les commandes. */
  headingHeight: number;
  pixelsPerBeat: number;
  /**
   * Faux quand aucune ligne d'automation n'est affichée.
   *
   * Le crayon a besoin d'une ligne sur laquelle écrire. Sans elle, montrer un
   * crayon promettrait un geste impossible : le corps du clip redevient alors
   * une prise pour le déplacer, et `VIEW` garde un rôle qu'on comprend.
   */
  canDraw: boolean;
}

/**
 * Ce qu'un appui à cet endroit déclencherait.
 *
 * L'ordre des questions est l'ordre des priorités, et il n'est pas
 * interchangeable : un bord reste un bord même s'il tombe dans la barre du
 * haut, sans quoi les sept pixels de prise du rognage disparaîtraient sur toute
 * la hauteur du titre.
 */
export function smartToolAt(clip: TrimmableClip, context: SmartToolContext): SmartTool {
  const edge = trimEdgeAtPointer(clip, context.beat, context.pixelsPerBeat);
  if (edge === "start") return "trim-start";
  if (edge === "end") return "trim-end";
  if (context.offsetY < context.headingHeight) return "move";
  return context.canDraw ? "draw" : "move";
}

/** La classe qui porte le curseur de cet outil, s'il en demande une. */
export function smartToolClass(tool: SmartTool): string {
  switch (tool) {
    case "trim-start":
      return "timeline-clip--trim-start";
    case "trim-end":
      return "timeline-clip--trim-end";
    case "draw":
      return "timeline-clip--draw";
    case "move":
      return "";
  }
}
