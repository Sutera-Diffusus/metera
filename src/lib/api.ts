import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { defaultSettings, type AgentActivity, type AppSettings, type AppStatus, type QuotaAccount, type ScanState, type UsageResponse } from "./types";
import { demoUsage } from "./demoData";
import { CNY_PER_USD } from "./pricing";
const native = () => "__TAURI_INTERNALS__" in window;
const previewSettings = () => {
  const query = new URLSearchParams(location.search);
  const metric = query.get("metric");
  return {
    ...defaultSettings,
    widgetMetric: metric === "cost" || metric === "quota" ? metric : "tokens",
    widgetCompact: query.get("compact") === "1",
  } satisfies AppSettings;
};
export const api = {
  status: (): Promise<AppStatus> => native() ? invoke("get_app_status") : Promise.resolve({ version: "1.7.0.1", dataDir: "local user data", sources: ["Codex","Claude Code","Kimi Code","WorkBuddy","ZCode","Reasonix","DeepSeek Harness"] }),
  usage: (start: string, end: string): Promise<UsageResponse> => native() ? invoke("fetch_usage", { start, end }) : Promise.resolve(demoUsage(start, end)),
  exchangeRate: (): Promise<number> => native() ? invoke<number>("get_exchange_rate") : Promise.resolve(CNY_PER_USD),
  activity: (): Promise<AgentActivity> => native() ? invoke("get_agent_activity") : Promise.resolve({ active: false, state:"idle", source: null, sources:[] }),
  quotas: (): Promise<QuotaAccount[]> => native() ? invoke("get_quota_status") : Promise.resolve([
    { provider:"codex",name:"ChatGPT / Codex",plan:"Plus",status:"connected",consuming:true,windows:[{label:"5 小时",usedPercent:42,remainingPercent:58},{label:"每周",usedPercent:27,remainingPercent:73}] },
    { provider:"kimi",name:"Kimi Code",plan:"高级套餐",status:"connected",consuming:false,windows:[{label:"5 小时",usedPercent:36,remainingPercent:64},{label:"每周",usedPercent:18,remainingPercent:82}] },
    { provider:"claude",name:"Claude Code",plan:"本地登录已检测",status:"connected",consuming:false,windows:[] },
    { provider:"workbuddy",name:"WorkBuddy",plan:"本地数据已检测",status:"connected",consuming:false,windows:[] }
  ]),
  bindAccount: (provider: string): Promise<void> => native() ? invoke("bind_account", { provider }) : Promise.resolve(),
  scanState: (): Promise<ScanState> => native() ? invoke("get_scan_state") : Promise.resolve({ status: "ready" }),
  settings: (): Promise<AppSettings> => native() ? invoke("get_settings") : Promise.resolve(previewSettings()),
  saveSettings: (settings: AppSettings) => native() ? invoke("set_settings", { settings }) : Promise.resolve(),
  setEmailSettings: (input: { enabled: boolean; email: string; smtpHost: string; smtpPort: number; smtpPassword: string }): Promise<void> => native() ? invoke("set_email_settings", { input }) : Promise.resolve(),
  sendTestEmail: (params: { email: string; smtpHost: string; smtpPort: number; smtpPassword: string }): Promise<string> => native() ? invoke("send_test_email", { email: params.email, smtpHost: params.smtpHost, smtpPort: params.smtpPort, smtpPassword: params.smtpPassword }) : Promise.resolve("预览模式：未真正发送"),
  sendReportNow: (): Promise<string> => native() ? invoke("send_report_now") : Promise.resolve("预览模式：未真正发送"),
  triggerScan: () => native() ? invoke("trigger_scan") : Promise.resolve(), showDashboard: () => native() ? invoke("show_dashboard") : Promise.resolve(),
  closeWidget: () => native() ? invoke("close_widget") : Promise.resolve(), collapseWidget: () => native() ? invoke("collapse_widget") : Promise.resolve(), expandWidget: () => native() ? invoke("expand_widget") : Promise.resolve(),
  startWidgetDrag: () => native() ? invoke("start_widget_drag") : Promise.resolve(),
  onScan: (handler: (s: ScanState) => void) => native() ? listen<ScanState>("scan-state", e => handler(e.payload)) : Promise.resolve(() => undefined), onSettings: (handler: (s: AppSettings) => void) => native() ? listen<AppSettings>("settings-updated", e => handler(e.payload)) : Promise.resolve(() => undefined),
  getLaunchAtLogin: (): Promise<boolean> => native() ? invoke("get_launch_at_login") : Promise.resolve(false), setLaunchAtLogin: (enabled:boolean) => native() ? invoke("set_launch_at_login", { enabled }) : Promise.resolve(), toggleWidget: () => native() ? invoke("toggle_widget") : Promise.resolve(), setWidgetCompact: (compact: boolean) => native() ? invoke("set_widget_compact", { compact }) : Promise.resolve(), quit: () => native() ? invoke("quit_app") : Promise.resolve()
};
