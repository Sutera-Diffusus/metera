import { useMemo, useState } from "react";
import { motion } from "motion/react";
import { displaySource, formatTokens } from "../../lib/analytics";
import type { Filters, UsageBucket } from "../../lib/types";
import { useMetera } from "../../state/MeteraContext";
import { modelBrandColor, sourceFill } from "../brands";

const OTHER = "__other";
const MAX_SOURCES = 7;
const MAX_MODELS = 8;

interface Flow { source: string; model: string; value: number }
interface Node { key: string; label: string; total: number; y: number; h: number }

const bucketTokens = (bucket: UsageBucket) =>
  bucket.inputTokens + bucket.cachedInputTokens + bucket.outputTokens + bucket.reasoningOutputTokens;

const flowId = (flow: Flow) => `${flow.source}|${flow.model}`;

// §17.6 工具与模型签名图形：桑基流图（源 → 模型，流宽 = Token，品牌色）。
// 数值一律伴随（顶部读数 / hover），不写在流带上。点击流或节点联动全局筛选。
export function SourceModelSankey({ buckets, filters, onFilters }: {
  buckets: UsageBucket[];
  filters: Filters;
  onFilters(filters: Filters): void;
}) {
  const state = useMetera();
  const [hover, setHover] = useState<string | null>(null); // "flow:s|m" | "source:s" | "model:m"

  const { flows, sources, models, empty } = useMemo(() => {
    const flowMap = new Map<string, Flow>();
    const sourceTotals = new Map<string, number>();
    const modelTotals = new Map<string, number>();
    for (const bucket of buckets) {
      const source = bucket.source || "unknown";
      const model = bucket.model || "unknown";
      const tokens = bucketTokens(bucket);
      if (!tokens) continue;
      const id = `${source}|${model}`; // 键只用于去重，永不解析
      const flow = flowMap.get(id) ?? { source, model, value: 0 };
      flow.value += tokens;
      flowMap.set(id, flow);
      sourceTotals.set(source, (sourceTotals.get(source) ?? 0) + tokens);
      modelTotals.set(model, (modelTotals.get(model) ?? 0) + tokens);
    }
    const rank = (map: Map<string, number>, limit: number) => {
      const sorted = [...map.entries()].sort((a, b) => b[1] - a[1]);
      const kept = sorted.slice(0, limit).map(([name]) => name);
      return { kept, hasOther: sorted.length > limit };
    };
    const rankedSources = rank(sourceTotals, MAX_SOURCES);
    const rankedModels = rank(modelTotals, MAX_MODELS);
    const sourceSet = new Set(rankedSources.kept);
    const modelSet = new Set(rankedModels.kept);

    // 归并「其他」后重聚合
    const merged = new Map<string, Flow>();
    for (const raw of flowMap.values()) {
      const source = sourceSet.has(raw.source) ? raw.source : OTHER;
      const model = modelSet.has(raw.model) ? raw.model : OTHER;
      const id = `${source}|${model}`;
      const flow = merged.get(id) ?? { source, model, value: 0 };
      flow.value += raw.value;
      merged.set(id, flow);
    }
    const flowList = [...merged.values()];
    const totals = (key: string, side: "source" | "model") =>
      flowList.filter(flow => flow[side] === key).reduce((sum, flow) => sum + flow.value, 0);
    const sourceKeys = [...rankedSources.kept, ...(rankedSources.hasOther ? [OTHER] : [])].filter(key => totals(key, "source") > 0);
    const modelKeys = [...rankedModels.kept, ...(rankedModels.hasOther ? [OTHER] : [])].filter(key => totals(key, "model") > 0);
    return { flows: flowList, empty: flowList.length === 0, sources: sourceKeys, models: modelKeys };
  }, [buckets]);

  const width = 920, height = 470, padY = 18, gap = 14, nodeW = 12;

  const layout = useMemo(() => {
    if (empty) return { sourceNodes: [] as Node[], modelNodes: [] as Node[], ribbons: new Map<string, string>() };
    const totals = (key: string, side: "source" | "model") =>
      flows.filter(flow => flow[side] === key).reduce((sum, flow) => sum + flow.value, 0);
    const totalAll = flows.reduce((sum, flow) => sum + flow.value, 0);
    // 两侧列总量相等，共用一条比例尺，流带才能恰好填满节点高度
    const scale = totalAll
      ? (height - padY * 2 - gap * Math.max(0, Math.max(sources.length, models.length) - 1)) / totalAll
      : 0;
    const place = (keys: string[], side: "source" | "model"): Node[] => {
      let cursor = padY;
      return keys.map(key => {
        const value = totals(key, side);
        const h = Math.max(4, value * scale);
        const node: Node = {
          key,
          label: key === OTHER ? "其他" : side === "source" ? displaySource(key) : key,
          total: value, y: cursor, h,
        };
        cursor += h + gap;
        return node;
      });
    };
    const sourceNodes = place(sources, "source");
    const modelNodes = place(models, "model");

    const x1 = nodeW, x2 = width - nodeW;
    const sourceUsed = new Map<string, number>(sourceNodes.map(node => [node.key, node.y]));
    const modelUsed = new Map<string, number>(modelNodes.map(node => [node.key, node.y]));
    const ribbons = new Map<string, string>();
    // 按源节点顺序铺流，保持两侧堆叠顺序一致
    const ordered = flows.slice().sort((a, b) =>
      sources.indexOf(a.source) - sources.indexOf(b.source) || models.indexOf(a.model) - models.indexOf(b.model));
    for (const flow of ordered) {
      const h = Math.max(1.2, flow.value * scale);
      const sy = sourceUsed.get(flow.source)!;
      const ty = modelUsed.get(flow.model)!;
      sourceUsed.set(flow.source, sy + h);
      modelUsed.set(flow.model, ty + h);
      const cx = (x1 + x2) / 2;
      ribbons.set(flowId(flow),
        `M${x1},${sy} C${cx},${sy} ${cx},${ty} ${x2},${ty} L${x2},${ty + h} C${cx},${ty + h} ${cx},${sy + h} ${x1},${sy + h} Z`);
    }
    return { sourceNodes, modelNodes, ribbons };
  }, [flows, sources, models, empty]);

  const hoverFlow = hover?.startsWith("flow:") ? flows.find(flow => `flow:${flowId(flow)}` === hover) ?? null : null;
  const dimFlow = (flow: Flow) => {
    if (!hover) return false;
    if (hoverFlow) return hoverFlow !== flow;
    if (hover.startsWith("source:")) return flow.source !== hover.slice(7);
    if (hover.startsWith("model:")) return flow.model !== hover.slice(6);
    return false;
  };
  const nodeActive = (side: "source" | "model", key: string) =>
    key !== OTHER && (side === "source" ? filters.source === key : filters.model === key);

  const selectFlow = (flow: Flow) => {
    const sameSource = flow.source === OTHER ? filters.source === "all" : filters.source === flow.source;
    const sameModel = flow.model === OTHER ? filters.model === "all" : filters.model === flow.model;
    onFilters({
      ...filters,
      source: sameSource && sameModel ? "all" : flow.source === OTHER ? "all" : flow.source,
      model: sameSource && sameModel ? "all" : flow.model === OTHER ? "all" : flow.model,
    });
  };
  const selectNode = (side: "source" | "model", key: string) => {
    if (key === OTHER) return;
    if (side === "source") onFilters({ ...filters, source: filters.source === key ? "all" : key });
    else onFilters({ ...filters, model: filters.model === key ? "all" : key });
  };

  const totalAll = flows.reduce((sum, flow) => sum + flow.value, 0);
  const readout = hoverFlow
    ? `${hoverFlow.source === OTHER ? "其他" : displaySource(hoverFlow.source)} → ${hoverFlow.model === OTHER ? "其他" : hoverFlow.model} · ${formatTokens(hoverFlow.value)}（${totalAll ? (hoverFlow.value / totalAll * 100).toFixed(1) : 0}%）`
    : hover?.startsWith("source:") && hover.slice(7) !== OTHER
      ? `${displaySource(hover.slice(7))} · ${formatTokens(layout.sourceNodes.find(node => node.key === hover.slice(7))?.total ?? 0)}`
      : hover?.startsWith("model:")
        ? `${hover.slice(6) === OTHER ? "其他模型" : hover.slice(6)} · ${formatTokens(layout.modelNodes.find(node => node.key === hover.slice(6))?.total ?? 0)}`
        : `${formatTokens(totalAll)} 流经 ${sources.length} 个工具 → ${models.length} 个模型`;

  if (empty) return <section className="v2-panel apple-sankey-panel"><header className="apple-panel-header"><h2>工具 → 模型 流量</h2></header><div className="v2-empty">当前范围暂无流向数据</div></section>;

  return <section className="v2-panel apple-sankey-panel">
    <header className="apple-panel-header"><h2>工具 → 模型 流量</h2></header>
    <div className="apple-sankey-readout" aria-live="polite"><span className="num">{readout}</span></div>
    <svg className="apple-sankey-svg" viewBox={`0 0 ${width} ${height}`} role="img" aria-label="工具到模型的 Token 流量桑基图" onMouseLeave={() => setHover(null)}>
      {flows.map((flow, index) => {
        const id = flowId(flow);
        const selected = flow.source !== OTHER && flow.model !== OTHER && filters.source === flow.source && filters.model === flow.model;
        return <motion.path key={`${state.chartReplay}-${id}`}
          className={`sankey-ribbon${dimFlow(flow) ? " dim" : ""}${selected ? " selected" : ""}`}
          d={layout.ribbons.get(id)} style={{ fill: sourceFill(flow.source) }}
          tabIndex={0} role="button"
          aria-label={`${flow.source === OTHER ? "其他" : displaySource(flow.source)} 到 ${flow.model === OTHER ? "其他" : flow.model}，${formatTokens(flow.value)}，Enter 联动筛选`}
          initial={state.chartReplayActive ? { opacity: 0 } : false} animate={{ opacity: 1 }}
          transition={{ duration: .5, delay: Math.min(index, 20) * .03 }}
          onMouseEnter={() => setHover(`flow:${id}`)} onFocus={() => setHover(`flow:${id}`)} onBlur={() => setHover(null)}
          onClick={() => selectFlow(flow)}
          onKeyDown={event => {
            if (event.key === "Enter" || event.key === " ") { event.preventDefault(); selectFlow(flow); }
            if (event.key === "Escape") setHover(null);
          }}/>;
      })}
      {layout.sourceNodes.map(node => <g key={node.key} className={`sankey-node${nodeActive("source", node.key) ? " active" : ""}`}
        tabIndex={0} role="button" aria-label={`工具 ${node.label}，${formatTokens(node.total)}，Enter 筛选`}
        onMouseEnter={() => setHover(`source:${node.key}`)} onFocus={() => setHover(`source:${node.key}`)} onBlur={() => setHover(null)}
        onClick={() => selectNode("source", node.key)}
        onKeyDown={event => { if (event.key === "Enter" || event.key === " ") { event.preventDefault(); selectNode("source", node.key); } }}>
        <rect x={0} y={node.y} width={nodeW} height={node.h} rx={4} style={{ fill: sourceFill(node.key) }}/>
        <text x={nodeW + 10} y={node.y + node.h / 2}>{node.label}</text>
      </g>)}
      {layout.modelNodes.map(node => <g key={node.key} className={`sankey-node${nodeActive("model", node.key) ? " active" : ""}`}
        tabIndex={0} role="button" aria-label={`模型 ${node.label}，${formatTokens(node.total)}，Enter 筛选`}
        onMouseEnter={() => setHover(`model:${node.key}`)} onFocus={() => setHover(`model:${node.key}`)} onBlur={() => setHover(null)}
        onClick={() => selectNode("model", node.key)}
        onKeyDown={event => { if (event.key === "Enter" || event.key === " ") { event.preventDefault(); selectNode("model", node.key); } }}>
        <rect x={width - nodeW} y={node.y} width={nodeW} height={node.h} rx={4} fill={node.key === OTHER ? "var(--brand-unknown)" : modelBrandColor(node.key)}/>
        <text x={width - nodeW - 10} y={node.y + node.h / 2} textAnchor="end">{node.label}</text>
      </g>)}
    </svg>
  </section>;
}
