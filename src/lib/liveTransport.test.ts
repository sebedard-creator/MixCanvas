import { describe, expect, it } from "vitest";

import { createLiveTransport } from "./liveTransport";
import type { TimelineTransportSnapshot } from "../timeline/types";

const STOPPED: TimelineTransportSnapshot = {
  status: "paused",
  positionBeat: 0,
  meterLeft: 0,
  meterRight: 0,
  meterOverload: false,
};

function at(positionBeat: number): TimelineTransportSnapshot {
  return { ...STOPPED, status: "playing", positionBeat };
}

describe("createLiveTransport", () => {
  it("gives the latest position to whoever asks, at any moment", () => {
    const transport = createLiveTransport(STOPPED);
    expect(transport.read().positionBeat).toBe(0);

    transport.publish(at(12.5));

    // Ce que lit une action déclenchée au clavier : la position d'maintenant,
    // pas celle du dernier rendu.
    expect(transport.read().positionBeat).toBe(12.5);
  });

  it("tells its subscribers, and stops once they leave", () => {
    const transport = createLiveTransport(STOPPED);
    const seen: number[] = [];
    const unsubscribe = transport.subscribe((snapshot) => seen.push(snapshot.positionBeat));

    transport.publish(at(1));
    transport.publish(at(2));
    unsubscribe();
    transport.publish(at(3));

    expect(seen).toEqual([1, 2]);
    // Parti ou non, l'abonné n'empêche pas la valeur d'avancer.
    expect(transport.read().positionBeat).toBe(3);
  });

  it("still serves the others when one leaves mid-delivery", () => {
    const transport = createLiveTransport(STOPPED);
    const seen: string[] = [];
    const unsubscribeFirst = transport.subscribe(() => {
      seen.push("first");
      unsubscribeFirst();
    });
    transport.subscribe(() => seen.push("second"));

    transport.publish(at(1));

    // Un panneau démonté pendant que le transport avance ne doit pas priver le
    // VU-mètre de sa mesure.
    expect(seen).toEqual(["first", "second"]);
  });
});
