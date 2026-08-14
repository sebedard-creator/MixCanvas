import { useEffect, useState } from "react";

import type { LiveTransport } from "../lib/liveTransport";
import { VU_RANGE_DB, vuDecibels, vuMeterPosition, vuSegmentZone } from "../lib/vuMeter";

interface StereoVuMeterProps {
  /** Le VU-mètre lit la lecture directement, plutôt que d'attendre un rendu. */
  liveTransport: LiveTransport;
}

const LED_SEGMENT_COUNT = 24;


/** Combien de diodes s'allument pour ce niveau. */
function litSegments(level: number): number {
  return level > 0.0001
    ? Math.max(1, Math.round(vuMeterPosition(level) * LED_SEGMENT_COUNT))
    : 0;
}

function LedVuMeter({ channel, level }: { channel: "L" | "R"; level: number }) {
  const activeSegments = litSegments(level);

  return (
    <div className="led-vu-channel">
      <span className="led-vu-channel-label" aria-hidden="true">{channel}</span>
      <div
        className="led-vu-track"
        role="meter"
        aria-label={`Master level ${channel === "L" ? "left" : "right"}`}
        aria-valuemin={VU_RANGE_DB.min}
        aria-valuemax={VU_RANGE_DB.max}
        aria-valuenow={Number(vuDecibels(level).toFixed(1))}
      >
        {Array.from({ length: LED_SEGMENT_COUNT }, (_, index) => (
          <div key={index} className="led-vu-socket">
            <i
              aria-hidden="true"
              className={`led-vu-segment led-vu-segment--${vuSegmentZone(index, LED_SEGMENT_COUNT)}${index < activeSegments ? " is-active" : ""}`}
            />
          </div>
        ))}
      </div>
    </div>
  );
}

export function StereoVuMeter({ liveTransport }: StereoVuMeterProps) {
  const [{ leftLevel, rightLevel, overload }, setReading] = useState(() => {
    const snapshot = liveTransport.read();
    return {
      leftLevel: snapshot.meterLeft,
      rightLevel: snapshot.meterRight,
      overload: snapshot.meterOverload,
    };
  });

  /**
   * Un rendu par diode qui change d'état, pas un par mesure reçue.
   *
   * Le niveau arrive vingt fois par seconde en valeur continue, mais
   * l'affichage n'a que vingt-quatre marches : la plupart de ces mesures ne
   * changeraient rien à l'écran. On ne se réveille que lorsqu'une diode
   * s'allume ou s'éteint — et le reste du panneau, lui, ne bouge plus du tout.
   */
  useEffect(() => {
    return liveTransport.subscribe((snapshot) => {
      setReading((current) => {
        const sameDisplay =
          litSegments(snapshot.meterLeft) === litSegments(current.leftLevel)
          && litSegments(snapshot.meterRight) === litSegments(current.rightLevel)
          && snapshot.meterOverload === current.overload;
        if (sameDisplay) return current;
        return {
          leftLevel: snapshot.meterLeft,
          rightLevel: snapshot.meterRight,
          overload: snapshot.meterOverload,
        };
      });
    });
  }, [liveTransport]);

  return (
    <div className="stereo-vu-meter" aria-label="Stereo master bus VU meter">
      <div className="led-vu-channels">
        <LedVuMeter channel="L" level={leftLevel} />
        <LedVuMeter channel="R" level={rightLevel} />
      </div>
      <div
        className="vu-overload-container"
        role="status"
        aria-label={overload ? "Master overload active" : "Master overload clear"}
        title="Master overload"
      >
        <span className="vu-overload-label">OL</span>
        <div className="vu-overload-socket">
          <i className={`vu-overload-led${overload ? " is-active" : ""}`} aria-hidden="true" />
        </div>
      </div>
    </div>
  );
}
