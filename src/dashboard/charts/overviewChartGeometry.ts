export interface ChartPoint { x: number; y: number; }

// Fritsch-Carlson style monotone cubic interpolation: smooth, but never invents
// a peak or dip between two measured points.
export function monotoneCurve(points: ChartPoint[]): string {
  if (points.length === 0) return "";
  if (points.length === 1) return `M ${points[0].x} ${points[0].y}`;

  const slopes: number[] = [];
  const tangents = Array.from({ length: points.length }, () => 0);
  for (let index = 0; index < points.length - 1; index += 1) {
    const dx = points[index + 1].x - points[index].x;
    slopes.push(dx === 0 ? 0 : (points[index + 1].y - points[index].y) / dx);
  }

  tangents[0] = slopes[0];
  tangents[tangents.length - 1] = slopes.at(-1) ?? 0;
  for (let index = 1; index < tangents.length - 1; index += 1) {
    const left = slopes[index - 1];
    const right = slopes[index];
    tangents[index] = left * right <= 0 ? 0 : (left + right) / 2;
  }

  for (let index = 0; index < slopes.length; index += 1) {
    const slope = slopes[index];
    if (slope === 0) {
      tangents[index] = 0;
      tangents[index + 1] = 0;
      continue;
    }
    const a = tangents[index] / slope;
    const b = tangents[index + 1] / slope;
    const magnitude = Math.hypot(a, b);
    if (magnitude > 3) {
      const scale = 3 / magnitude;
      tangents[index] = scale * a * slope;
      tangents[index + 1] = scale * b * slope;
    }
  }

  let path = `M ${points[0].x} ${points[0].y}`;
  for (let index = 0; index < points.length - 1; index += 1) {
    const current = points[index];
    const next = points[index + 1];
    const dx = (next.x - current.x) / 3;
    path += ` C ${current.x + dx} ${current.y + tangents[index] * dx}, ${next.x - dx} ${next.y - tangents[index + 1] * dx}, ${next.x} ${next.y}`;
  }
  return path;
}

export const lerp = (from: number, to: number, amount: number) => from + (to - from) * amount;
