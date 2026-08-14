import { RotateCcw } from "lucide-react";
import { useEffect, useRef, useState } from "react";
import { MotionConfig, motion, useAnimationControls } from "motion/react";
import { WindowChrome } from "./components/WindowChrome";
import type { DashboardTheme } from "./components/ThemeToggle";
import { api } from "./lib/api";
import { useMetera } from "./state/MeteraContext";
import { DashboardControls } from "./dashboard/DashboardControls";
import { DashboardSidebar, type DashboardView } from "./dashboard/DashboardSidebar";
import { AccountsView } from "./dashboard/views/AccountsView";
import { InsightsView } from "./dashboard/views/InsightsView";
import { OverviewView } from "./dashboard/views/OverviewView";
import { ProvidersView } from "./dashboard/views/ProvidersView";
import { SessionsView } from "./dashboard/views/SessionsView";
import { SettingsView } from "./dashboard/views/SettingsView";
import { ToolsView } from "./dashboard/views/ToolsView";
import { UsageView } from "./dashboard/views/UsageView";
import "./dashboard/dashboard-v2.css";
import "./dashboard/apple-tokens.css";
import "./dashboard/apple.css";

const filterLabels = { hostname: "终端", source: "工具", provider: "供应商", model: "模型", project: "项目" } as const;

// 开发环境截图/验收工具：`?view=<视图>&theme=day|moon` 覆盖初始视图与主题（生产构建剥离）
const devOverrides = import.meta.env.DEV ? new URLSearchParams(window.location.search) : null;
const devView = (value: string | null): DashboardView | null =>
  value && ["overview", "usage", "insights", "tools", "providers", "accounts", "sessions", "settings"].includes(value) ? value as DashboardView : null;

export function DashboardApp() {
  const state = useMetera();
  const [view, setView] = useState<DashboardView>(() => devView(devOverrides?.get("view") ?? null) ?? "overview");
  const [sidebarCollapsed, setSidebarCollapsed] = useState(() => matchMedia("(max-width: 1100px)").matches);
  useEffect(() => {
    const media = matchMedia("(max-width: 1100px)");
    const sync = () => setSidebarCollapsed(media.matches);
    media.addEventListener("change", sync);
    return () => media.removeEventListener("change", sync);
  }, []);
  const [theme, setTheme] = useState<DashboardTheme>(() => {
    const override = devOverrides?.get("theme");
    if (override === "day" || override === "moon") return override;
    const saved = localStorage.getItem("metera-dashboard-theme");
    if (saved === "day" || saved === "moon") return saved;
    return matchMedia("(prefers-color-scheme: light)").matches ? "day" : "moon";
  });
  const mainRef = useRef<HTMLElement>(null);
  const switching = useRef(false);
  const currentView = useRef<DashboardView>(view);
  const desiredView = useRef<DashboardView>(view);
  const viewControls = useAnimationControls();
  const activeFilters = Object.entries(state.filters).filter(([, value]) => value !== "all") as Array<[keyof typeof state.filters, string]>;
  const showControls = !["overview", "accounts", "settings"].includes(view);

  const runViewTransition = async () => {
    if (switching.current) return;
    switching.current = true;
    try {
      while (desiredView.current !== currentView.current) {
        const target = desiredView.current;
        currentView.current = target;
        setView(target);
        await Promise.race([
          viewControls.start({ scale: .992, y: 2, filter: "blur(0.8px)", transition: { duration: .11, ease: [0.4, 0, 1, 1] } }).catch(() => undefined),
          new Promise(resolve => window.setTimeout(resolve, 90)),
        ]);
        await Promise.race([
          viewControls.start({ scale: 1, y: 0, filter: "blur(0px)", transition: { type: "spring", stiffness: 520, damping: 38, mass: .55 } }).catch(() => undefined),
          new Promise(resolve => window.setTimeout(resolve, 260)),
        ]);
      }
    } finally {
      switching.current = false;
      if (desiredView.current !== currentView.current) void runViewTransition();
    }
  };

  const changeView = (next: DashboardView) => {
    desiredView.current = next;
    if (next !== currentView.current) void runViewTransition();
  };

  const content = {
    overview: <OverviewView onNavigate={changeView}/>, usage: <UsageView/>, insights: <InsightsView/>, tools: <ToolsView/>, providers: <ProvidersView/>,
    accounts: <AccountsView/>, sessions: <SessionsView/>, settings: <SettingsView/>,
  }[view];

  const trackSurfacePointer = (event: React.PointerEvent<HTMLElement>) => {
    const surface = (event.target as HTMLElement).closest<HTMLElement>(".v2-panel, .v2-metric-band, .v2-account-card");
    if (!surface) return;
    const bounds = surface.getBoundingClientRect();
    surface.style.setProperty("--pointer-x", `${event.clientX - bounds.left}px`);
    surface.style.setProperty("--pointer-y", `${event.clientY - bounds.top}px`);
  };

  const toggleTheme = () => {
    const next = theme === "day" ? "moon" : "day";
    localStorage.setItem("metera-dashboard-theme", next);
    setTheme(next);
  };

  return <MotionConfig reducedMotion="user">
  <div className="dashboard-shell v2-dashboard-shell" data-theme={theme}>
    <WindowChrome theme={theme} onThemeToggle={toggleTheme}/>
    <div className="v2-workspace">
      <DashboardSidebar view={view} collapsed={sidebarCollapsed} onView={changeView} onCollapse={() => setSidebarCollapsed(value => !value)} activity={state.activity} scan={state.scan}/>
      <main ref={mainRef} className="v2-dashboard-main" onPointerMove={trackSurfacePointer}>
        {state.loading && <div className="v2-loading-state" role="status" aria-live="polite"><i aria-hidden="true"/>正在读取本地数据</div>}
        {showControls && <DashboardControls aliases={state.settings.providerAliases} state={{
          rawBuckets: state.rawBuckets, range: state.range, filters: state.filters, customStart: state.customStart, customEnd: state.customEnd,
          scanning: state.scan.status === "scanning", setRange: state.setRange, setFilters: state.setFilters,
          setCustomStart: state.setCustomStart, setCustomEnd: state.setCustomEnd, refresh: () => void api.triggerScan(),
        }}/>}
        {activeFilters.length > 0 && showControls && <div className="v2-filter-summary" aria-label="已启用筛选"><span>筛选中</span>{activeFilters.map(([field, value]) => <button key={field} onClick={() => state.setFilters({ ...state.filters, [field]: "all" })}>{filterLabels[field]}：{value}<b>×</b></button>)}<button className="reset" onClick={() => state.setFilters({ hostname: "all", source: "all", provider: "all", model: "all", project: "all" })}><RotateCcw/>清除全部</button></div>}
        {state.error && <div className="v2-error" role="alert">数据读取失败：{state.error}</div>}
        <motion.div className="v2-view-stage" animate={viewControls} initial={false}>
          <motion.div
            className="v2-view-transition"
            key={view}
            initial={false}
            animate={{ opacity: 1, y: 0, filter: "blur(0px)" }}
            transition={{ duration: .24, ease: [0.22, 1, 0.36, 1] }}
          >
            {content}
          </motion.div>
        </motion.div>
      </main>
    </div>
  </div>
  </MotionConfig>;
}
