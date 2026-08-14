import { Minus, Square, X } from "lucide-react";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { ThemeToggle, type DashboardTheme } from "./ThemeToggle";
import meteraMark from "../assets/brands/metera-mark.svg";

export function WindowChrome({ theme = "moon", onThemeToggle }: { theme?: DashboardTheme; onThemeToggle?(): void }) {
  const current="__TAURI_INTERNALS__" in window?getCurrentWindow():null;
  return <div className="window-chrome" data-tauri-drag-region>
    <div className="wordmark" data-tauri-drag-region><img className="mark" src={meteraMark} alt="" width={28} height={28} draggable={false}/><strong data-tauri-drag-region>Metera</strong><small data-tauri-drag-region>Usage intelligence</small></div>
    {onThemeToggle && <ThemeToggle theme={theme} onToggle={onThemeToggle}/>}
    <div className="window-actions"><button aria-label="最小化" title="最小化" onClick={()=>void current?.minimize()}><Minus/></button><button aria-label="最大化" title="最大化" onClick={()=>void current?.toggleMaximize()}><Square/></button><button aria-label="关闭仪表盘" title="关闭仪表盘" onClick={()=>void current?.close()}><X/></button></div>
  </div>;
}
