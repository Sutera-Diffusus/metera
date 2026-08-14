import { useEffect, useState, type CSSProperties } from "react";

const DIGITS = Array.from({ length: 10 }, (_, digit) => digit);

export type RollingGlyph = { kind: "digit"; value: number } | { kind: "symbol"; value: string };

export function splitRollingValue(value: string): RollingGlyph[] {
  return Array.from(value, character => /[0-9]/.test(character)
    ? { kind: "digit", value: Number(character) }
    : { kind: "symbol", value: character });
}

export function RollingNumber({ value, className = "" }: { value: string; className?: string }) {
  const [ready, setReady] = useState(false);

  useEffect(() => {
    const frame = requestAnimationFrame(() => setReady(true));
    return () => cancelAnimationFrame(frame);
  }, []);

  return <span className={`rolling-number ${className}`.trim()} aria-label={value}>
    <span className="rolling-number-visual" aria-hidden="true">
      {splitRollingValue(value).map((glyph, index) => glyph.kind === "digit"
        ? <span className="rolling-digit" key={index}>
            <span
              className="rolling-digit-track"
              style={{
                transform: `translate3d(0, -${(ready ? glyph.value : 0) * 10}%, 0)`,
                "--rolling-delay": `${Math.min(index, 7) * 18}ms`,
              } as CSSProperties}
            >
              {DIGITS.map(digit => <i key={digit}>{digit}</i>)}
            </span>
          </span>
        : <span className="rolling-symbol" key={index}>{glyph.value}</span>)}
    </span>
  </span>;
}
