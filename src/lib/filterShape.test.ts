import { describe, expect, it } from "vitest";
import {
  FILTER_BUBBLE_BYPASS_EPSILON_BEATS,
  FILTER_STROKE_MAX_SAMPLES,
  filterBubblePoints,
  filterShapeMultiplier,
  filterStrokeNodes,
} from "./filterShape";

describe("filterShapeMultiplier", () => {
  it("ramps up from bypass to the full value", () => {
    expect(filterShapeMultiplier("ramp_up", 0)).toBe(0);
    expect(filterShapeMultiplier("ramp_up", 0.5)).toBe(0.5);
    expect(filterShapeMultiplier("ramp_up", 1)).toBe(1);
  });

  it("ramps down from the full value back to bypass", () => {
    expect(filterShapeMultiplier("ramp_down", 0)).toBe(1);
    expect(filterShapeMultiplier("ramp_down", 0.5)).toBe(0.5);
    expect(filterShapeMultiplier("ramp_down", 1)).toBe(0);
  });

  it("peaks in the middle of a symmetric triangle", () => {
    expect(filterShapeMultiplier("triangle", 0)).toBe(0);
    expect(filterShapeMultiplier("triangle", 0.5)).toBe(1);
    expect(filterShapeMultiplier("triangle", 1)).toBe(0);
  });
});

describe("filterBubblePoints", () => {
  const bubble = { startBeat: 24, widthBeats: 4, value: -0.8 };

  it("samples a ramp_up across its width and closes it on bypass", () => {
    const points = filterBubblePoints({ ...bubble, shape: "ramp_up" });

    expect(points[0]).toEqual({ beat: 24, value: -0 });
    const middle = points.find((point) => point.beat === 26);
    expect(middle?.value).toBeCloseTo(-0.4, 10);

    const last = points[points.length - 1];
    expect(last.beat).toBeCloseTo(28 + FILTER_BUBBLE_BYPASS_EPSILON_BEATS, 10);
    expect(last.value).toBe(0);

    const edge = points[points.length - 2];
    expect(edge.beat).toBe(28);
    expect(edge.value).toBeCloseTo(-0.8, 10);
  });

  it("opens a ramp_down on bypass before its first beat", () => {
    const points = filterBubblePoints({ ...bubble, shape: "ramp_down" });

    expect(points[0].beat).toBeCloseTo(24 - FILTER_BUBBLE_BYPASS_EPSILON_BEATS, 10);
    expect(points[0].value).toBe(0);
    expect(points[1].value).toBeCloseTo(-0.8, 10);
    expect(points[points.length - 1]).toEqual({ beat: 28, value: -0 });
  });

  it("never places a bypass sample before the start of the project", () => {
    const points = filterBubblePoints({ startBeat: 0, widthBeats: 4, value: 0.5, shape: "ramp_down" });
    expect(points[0].beat).toBe(0);
  });

  it("leaves a triangle to close on its own edges", () => {
    const points = filterBubblePoints({ ...bubble, shape: "triangle" });

    expect(points[0].beat).toBe(24);
    expect(points[0].value).toBe(-0);
    expect(points[points.length - 1].beat).toBe(28);
    expect(points[points.length - 1].value).toBe(-0);
    const peak = points.find((point) => point.beat === 26);
    expect(peak?.value).toBeCloseTo(-0.8, 10);
  });

  it("defaults to ramp_up when no shape is given, matching the Rust default", () => {
    expect(filterBubblePoints(bubble)).toEqual(
      filterBubblePoints({ ...bubble, shape: "ramp_up" }),
    );
  });
});

describe("filterStrokeNodes", () => {
  it("remet les quarts peints dans l'ordre du temps", () => {
    // La main peut revenir en arrière et repasser sur ce qu'elle a déjà peint :
    // c'est la dernière valeur qui vaut, et la courbe reste croissante.
    const painted = new Map([
      [8.5, 0.4],
      [8, 0.2],
      [8.25, -0.6],
    ]);
    const nodes = filterStrokeNodes(painted);
    const beats = nodes.map(([beat]) => beat);
    expect([...beats]).toEqual([...beats].sort((left, right) => left - right));
    expect(nodes).toContainEqual([8.25, -0.6]);
  });

  it("referme le trait au bypass de part et d'autre", () => {
    const nodes = filterStrokeNodes(new Map([[8, 0.8], [12, 0.8]]));
    expect(nodes[0]).toEqual([8 - FILTER_BUBBLE_BYPASS_EPSILON_BEATS, 0]);
    expect(nodes[nodes.length - 1]).toEqual([12 + FILTER_BUBBLE_BYPASS_EPSILON_BEATS, 0]);
  });

  it("ne pose pas d'ancre avant le début de la timeline", () => {
    const nodes = filterStrokeNodes(new Map([[0, 0.5]]));
    expect(nodes[0]).toEqual([0, 0.5]);
    expect(nodes.every(([beat]) => beat >= 0)).toBe(true);
  });

  it("ne dessine rien pour un trait vide", () => {
    expect(filterStrokeNodes(new Map())).toEqual([]);
  });

  it("reste sous le plafond que le serveur accepte", () => {
    const painted = new Map<number, number>();
    for (let step = 0; step < FILTER_STROKE_MAX_SAMPLES * 2; step += 1) {
      painted.set(step * 0.25, 0.5);
    }
    const nodes = filterStrokeNodes(painted);
    expect(nodes.length).toBeLessThanOrEqual(FILTER_STROKE_MAX_SAMPLES);
  });
});
