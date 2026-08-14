import { useState } from "react";
import { formatTokens } from "../../lib/analytics";
import { AnimatePresence, motion } from "motion/react";
import { usePhysicalTilt } from "../../hooks/usePhysicalTilt";
import { useMetera } from "../../state/MeteraContext";

type Kind = "input" | "cache" | "output" | "reasoning";
type DotKind = Kind | "empty";
const labels: Record<Kind, string> = { input: "输入", cache: "缓存", output: "输出", reasoning: "推理" };

export function TokenWaffle({ input, cache, output, reasoning }: { input: number; cache: number; output: number; reasoning: number }) {
  const state = useMetera();
  const [selected, setSelected] = useState<Kind | null>(null);
  const values: Record<Kind, number> = { input, cache, output, reasoning };
  const total = input + cache + output + reasoning;
  const counts = (Object.keys(values) as Kind[]).map(kind => ({ kind, count: total ? Math.round(values[kind] / total * 100) : 0 }));
  const difference = 100 - counts.reduce((sum, item) => sum + item.count, 0);
  if (counts.length) counts[0].count += difference;
  const dots: DotKind[] = total
    ? counts.flatMap(item => Array.from({ length: Math.max(0, item.count) }, () => item.kind)).slice(0, 100)
    : Array.from({ length: 100 }, () => "empty" as const);
  const selectedValue = selected ? values[selected] : total;
  const physical = usePhysicalTilt(1);
  return <motion.section layout className="v2-panel v2-waffle-panel" style={physical.style} onPointerMove={physical.onPointerMove} onPointerLeave={physical.onPointerLeave} whileHover={{ y: -5, scale: 1.004 }} whileTap={{ scale: .994 }} transition={{ type: "spring", stiffness: 320, damping: 30, mass: .8 }}>
    <header className="v2-panel-header"><div><h2>Token 构成</h2><p>每个圆点约代表当前总量的 1%</p></div><div className="v2-waffle-total"><strong>{formatTokens(selectedValue)}</strong><span>{selected ? labels[selected] : "总 Token"}</span></div></header>
    <div className="v2-waffle" role="img" aria-label={total ? `Token 构成，总计 ${formatTokens(total)}` : "Token 构成暂无数据"}>{dots.map((kind, index) => <motion.i initial={state.chartReplayActive ? { scale: 0, opacity: 0 } : false} animate={{ opacity: selected && selected !== kind ? .12 : 1, scale: selected && selected !== kind ? .68 : 1 }} transition={{ type: "spring", stiffness: 540, damping: 30, mass: .35, delay: Math.min(index, 30) * .008 }} className={kind} key={`${state.chartReplay}-${index}`}/>)}</div>
    <div className="v2-waffle-legend">{(Object.keys(values) as Kind[]).map((kind, index) => <motion.button layout className={selected === kind ? "active" : ""} aria-pressed={selected === kind} onClick={() => setSelected(selected === kind ? null : kind)} key={kind} whileHover={{ y: -4, rotate: index % 2 ? .35 : -.35 }} whileTap={{ scale: .93, y: 1 }} transition={{ type: "spring", stiffness: 430, damping: 26, mass: .55 }}><i className={kind}/><span>{labels[kind]}</span><strong>{formatTokens(values[kind])}</strong><small>{total ? `${(values[kind] / total * 100).toFixed(1)}%` : "0%"}</small>{selected === kind && <AnimatePresence><motion.em initial={{ scaleX: 0 }} animate={{ scaleX: 1 }} exit={{ scaleX: 0 }} transition={{ type: "spring", stiffness: 420, damping: 28 }}/></AnimatePresence>}</motion.button>)}</div>
  </motion.section>;
}
