import { vuDecibels, vuMeterPosition, vuSegmentZone } from "../lib/vuMeter";

interface StereoVuMeterProps {
  leftLevel: number;
  rightLevel: number;
  overload: boolean;
}

const LED_SEGMENT_COUNT = 24;


function LedVuMeter({ channel, level }: { channel: "L" | "R"; level: number }) {
  const activeSegments = level > 0.0001
    ? Math.max(1, Math.round(vuMeterPosition(level) * LED_SEGMENT_COUNT))
    : 0;

  return (
    <div className="led-vu-channel">
      <span className="led-vu-channel-label" aria-hidden="true">{channel}</span>
      <div
        className="led-vu-track"
        role="meter"
        aria-label={`Master level ${channel === "L" ? "left" : "right"}`}
        aria-valuemin={-20}
        aria-valuemax={3}
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

export function StereoVuMeter({ leftLevel, rightLevel, overload }: StereoVuMeterProps) {
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
