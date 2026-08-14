import {
  Activity,
  BarChart3,
  ChartNoAxesCombined,
  Bot,
  ChevronLeft,
  Gauge,
  Globe2,
  LayoutDashboard,
  Settings,
  TerminalSquare,
} from "lucide-react";
import { motion } from "motion/react";
import { useCallback, useLayoutEffect, useRef, useState } from "react";
import meteraMark from "../assets/brands/metera-mark.svg";
import type { ScanState } from "../lib/types";

export type DashboardView = "overview" | "usage" | "insights" | "tools" | "providers" | "accounts" | "sessions" | "settings";

interface NavItem { id: DashboardView; label: string; icon: typeof Activity }

// §17.5 骨架：分组式侧边栏（监控 / 管理 / 系统）
const groups: Array<{ title: string; items: NavItem[] }> = [
  {
    title: "监控",
    items: [
      { id: "overview", label: "总览", icon: LayoutDashboard },
      { id: "usage", label: "用量分析", icon: BarChart3 },
      { id: "insights", label: "深度分析", icon: ChartNoAxesCombined },
    ],
  },
  {
    title: "管理",
    items: [
      { id: "tools", label: "工具与模型", icon: Bot },
      { id: "providers", label: "供应商", icon: Globe2 },
      { id: "accounts", label: "额度与账号", icon: Gauge },
      { id: "sessions", label: "会话记录", icon: TerminalSquare },
    ],
  },
  {
    title: "系统",
    items: [{ id: "settings", label: "设置", icon: Settings }],
  },
];

function syncLine(scan: ScanState): { text: string; tone: "ok" | "busy" | "error" } {
  if (scan.status === "scanning") return { text: "正在同步数据源…", tone: "busy" };
  if (scan.status === "error") return { text: scan.message ?? "同步异常", tone: "error" };
  return { text: "所有数据源正常", tone: "ok" };
}

export function DashboardSidebar({
  view,
  collapsed,
  onView,
  onCollapse,
  activity,
  scan,
}: {
  view: DashboardView;
  collapsed: boolean;
  onView(view: DashboardView): void;
  onCollapse(): void;
  activity: { state: "idle" | "active" | "waiting" | "error"; sources: string[] };
  scan: ScanState;
}) {
  const navRef = useRef<HTMLElement>(null);
  const navButtons = useRef(new Map<DashboardView, HTMLButtonElement>());
  const [navSlider, setNavSlider] = useState({ top: 0, left: 0, width: 0, height: 0, visible: false });
  const measureNavSlider = useCallback(() => {
    const nav = navRef.current;
    const target = navButtons.current.get(view);
    if (!nav || !target) return;
    const navRect = nav.getBoundingClientRect();
    const targetRect = target.getBoundingClientRect();
    setNavSlider({ top: targetRect.top - navRect.top, left: targetRect.left - navRect.left, width: targetRect.width, height: targetRect.height, visible: true });
  }, [view, collapsed]);
  useLayoutEffect(() => {
    measureNavSlider();
    const observer = typeof ResizeObserver === "undefined" ? null : new ResizeObserver(measureNavSlider);
    if (observer && navRef.current) observer.observe(navRef.current);
    window.addEventListener("resize", measureNavSlider);
    return () => {
      observer?.disconnect();
      window.removeEventListener("resize", measureNavSlider);
    };
  }, [measureNavSlider]);
  const statusText = activity.state === "active" ? "Agent 正在运行" : activity.state === "waiting" ? "等待确认" : activity.state === "error" ? "检测到异常" : "当前待机";
  const sync = syncLine(scan);
  return <aside className={`v2-sidebar apple-sidebar${collapsed ? " collapsed" : ""}`}>
    <div className="apple-brand" aria-hidden="true">
      <img className="apple-brand-mark" src={meteraMark} alt="" width={30} height={30} draggable={false}/>
      {!collapsed && <strong>Metera</strong>}
    </div>
    <nav ref={navRef} aria-label="仪表盘导航">
      {groups.map(group => <div className="apple-nav-group" key={group.title}>
        {!collapsed && <h3>{group.title}</h3>}
        {group.items.map(item => {
          const Icon = item.icon;
          const active = view === item.id;
          return <motion.button key={item.id} ref={node => { if (node) navButtons.current.set(item.id, node); else navButtons.current.delete(item.id); }} className={active ? "active" : ""} onClick={() => onView(item.id)} aria-current={active ? "page" : undefined} title={collapsed ? item.label : undefined}
            whileTap={{ scale: .96 }} transition={{ type: "spring", stiffness: 470, damping: 30, mass: .55 }}>
            <Icon/><span>{item.label}</span>
          </motion.button>;
        })}
      </div>)}
      {navSlider.visible && <motion.i className="apple-nav-slider" aria-hidden="true" style={{ top: navSlider.top, left: navSlider.left, width: navSlider.width, height: navSlider.height }}/>}
    </nav>
    <div className={`apple-sync-card ${activity.state}`} title={`${statusText} · ${sync.text}`}>
      <div className="apple-sync-top">
        <i className={`apple-sync-dot ${activity.state}`}/>
        {!collapsed && <strong>{statusText}</strong>}
      </div>
      {!collapsed && <small className={`apple-sync-line ${sync.tone}`}>{sync.text}{activity.sources.length ? ` · ${activity.sources.join(" · ")}` : ""}</small>}
    </div>
    <button className="v2-collapse" onClick={onCollapse} aria-label={collapsed ? "展开侧边栏" : "收起侧边栏"} title={collapsed ? "展开侧边栏" : "收起侧边栏"}><ChevronLeft/></button>
  </aside>;
}
