import { CalendarRange, Check, ChevronDown, SlidersHorizontal } from "lucide-react";
import type { Filters, RangeKey, UsageBucket } from "../lib/types";
import { displaySource } from "../lib/analytics";
import { AnimatePresence, motion } from "motion/react";
import { useEffect, useRef, useState } from "react";

const ranges: [RangeKey, string][] = [["today", "今天"], ["24h", "近 24 小时"], ["7d", "近 7 天"], ["30d", "近 30 天"], ["90d", "近 90 天"], ["custom", "自定义"]];
const rangeLabel = (key: RangeKey) => ranges.find(([id]) => id === key)?.[1] ?? key;
const filterLabels: Record<keyof Filters, string> = { hostname: "终端", source: "工具", provider: "供应商", model: "模型", project: "项目" };

// §17.5 筛选工具栏：默认收起为「范围 chip + 筛选按钮」，展开后才出现完整控件。
export function DashboardControls({ state, aliases }: {
  state: {
    rawBuckets: UsageBucket[]; range: RangeKey; filters: Filters; customStart: string; customEnd: string;
    scanning: boolean; setRange(value: RangeKey): void; setFilters(value: Filters): void;
    setCustomStart(value: string): void; setCustomEnd(value: string): void; refresh(): void;
  };
  aliases: Record<string, string>;
}) {
  const [openPanel, setOpenPanel] = useState<"range" | "filter" | null>(null);
  const controlsRef = useRef<HTMLElement>(null);
  useEffect(() => {
    const closeOutside = (event: PointerEvent) => {
      if (!controlsRef.current?.contains(event.target as Node)) setOpenPanel(null);
    };
    const closeOnEscape = (event: KeyboardEvent) => {
      if (event.key === "Escape") setOpenPanel(null);
    };
    document.addEventListener("pointerdown", closeOutside);
    document.addEventListener("keydown", closeOnEscape);
    return () => {
      document.removeEventListener("pointerdown", closeOutside);
      document.removeEventListener("keydown", closeOnEscape);
    };
  }, []);

  const unique = (field: keyof UsageBucket) => [...new Set(state.rawBuckets.map(row => String(row[field] || "unknown")))].sort();
  const configs: Array<{ field: keyof Filters; values: string[]; format?: (value: string) => string }> = [
    { field: "hostname", values: unique("hostname") },
    { field: "source", values: unique("source"), format: displaySource },
    { field: "provider", values: unique("provider"), format: value => aliases[value] ?? value },
    { field: "model", values: unique("model") },
    { field: "project", values: unique("project") },
  ];
  const activeCount = Object.values(state.filters).filter(value => value !== "all").length;

  return <section ref={controlsRef} className="v2-controls apple-controls" aria-label="数据范围与筛选">
    <div className="apple-control-cluster">
      <button type="button" className={`apple-chip${openPanel === "range" ? " open" : ""}`} aria-haspopup="listbox" aria-expanded={openPanel === "range"} onClick={() => setOpenPanel(openPanel === "range" ? null : "range")}>
        <CalendarRange/><span>{rangeLabel(state.range)}</span><ChevronDown/>
      </button>
      <AnimatePresence>{openPanel === "range" && <motion.div className="apple-popover apple-range-popover" role="listbox" aria-label="时间范围" initial={{ opacity: 0, y: -6, scale: .98 }} animate={{ opacity: 1, y: 0, scale: 1 }} exit={{ opacity: 0, y: -4, scale: .98 }} transition={{ duration: .14, ease: [0.16, 1, 0.3, 1] }}>
        {ranges.map(([key, label]) => {
          const active = state.range === key;
          return <button type="button" role="option" aria-selected={active} className={active ? "active" : ""} key={key} onClick={() => { state.setRange(key); if (key !== "custom") setOpenPanel(null); }}><span>{label}</span>{active && <Check/>}</button>;
        })}
        {state.range === "custom" && <div className="apple-custom-range">
          <input aria-label="开始日期" type="date" value={state.customStart} onChange={event => state.setCustomStart(event.target.value)}/>
          <span>至</span>
          <input aria-label="结束日期" type="date" value={state.customEnd} onChange={event => state.setCustomEnd(event.target.value)}/>
        </div>}
      </motion.div>}</AnimatePresence>
    </div>

    <div className="apple-control-cluster">
      <button type="button" className={`apple-chip${openPanel === "filter" ? " open" : ""}${activeCount ? " filtered" : ""}`} aria-haspopup="dialog" aria-expanded={openPanel === "filter"} onClick={() => setOpenPanel(openPanel === "filter" ? null : "filter")}>
        <SlidersHorizontal/><span>筛选</span>{activeCount > 0 && <b>{activeCount}</b>}<ChevronDown/>
      </button>
      <AnimatePresence>{openPanel === "filter" && <motion.div className="apple-popover apple-filter-popover" role="dialog" aria-label="数据筛选" initial={{ opacity: 0, y: -6, scale: .98 }} animate={{ opacity: 1, y: 0, scale: 1 }} exit={{ opacity: 0, y: -4, scale: .98 }} transition={{ duration: .14, ease: [0.16, 1, 0.3, 1] }}>
        {configs.map(config => {
          const selected = state.filters[config.field];
          return <label className="apple-filter-row" key={config.field}>
            <span>{filterLabels[config.field]}</span>
            <select value={selected} onChange={event => state.setFilters({ ...state.filters, [config.field]: event.target.value })}>
              <option value="all">全部</option>
              {config.values.map(value => <option key={value} value={value}>{config.format ? config.format(value) : value}</option>)}
            </select>
          </label>;
        })}
        {activeCount > 0 && <button type="button" className="apple-filter-reset" onClick={() => state.setFilters({ hostname: "all", source: "all", provider: "all", model: "all", project: "all" })}>清除全部筛选</button>}
      </motion.div>}</AnimatePresence>
    </div>

  </section>;
}
