export interface UsageBucket {
  source: string; provider: string; model: string; project: string; hostname: string; bucketStart: string;
  inputTokens: number; outputTokens: number; cachedInputTokens: number; reasoningOutputTokens: number; totalTokens: number;
  estimatedCost?: number | null;
}
export interface UsageSession {
  source: string; project: string; hostname: string; sessionHash: string;
  firstMessageAt: string; lastMessageAt: string; durationSeconds: number; activeSeconds: number;
  messageCount: number; userMessageCount: number; userPromptHours: number[];
}
export interface UsageResponse { buckets: UsageBucket[]; sessions: UsageSession[]; hasAnyData: boolean; }
export type RangeKey = "today" | "24h" | "7d" | "30d" | "90d" | "custom";
export interface Filters { hostname: string; source: string; provider: string; model: string; project: string; }
export interface AppSettings {
  widgetVisible: boolean; widgetAlwaysOnTop: boolean; widgetCompact: boolean; widgetX: number | null; widgetY: number | null;
  widgetMetric: "tokens" | "cost" | "quota"; widgetCollapsed: boolean; widgetEdge: string; pinnedQuotaProviders: string[];
  providerAliases: Record<string,string>;
  planOverrides: Record<string,string>;
  accent: "mint" | "blue" | "coral"; density: "comfortable" | "compact"; includeProjectNames: boolean;
  showCostInTray: boolean; showTokensInTray: boolean;
  dailyReportEnabled: boolean; reportEmail: string; reportSmtpHost: string; reportSmtpPort: number; reportSmtpPassword: string;
  reportSendTime: string; reportLastSentAt: string | null; reportLastError: string | null;
}
export interface ScanState { status: "idle" | "scanning" | "ready" | "error"; message?: string | null; lastScanAt?: number | null; }
export interface AppStatus { version: string; dataDir: string; sources: string[]; }
export type AgentState = "idle" | "active" | "waiting" | "error";
export interface AgentActivity { active: boolean; state: AgentState; source: string | null; sources: string[]; detail?: string | null; }
export interface QuotaWindow { kind?: "primary" | "secondary" | null; label: string; usedPercent: number | null; remainingPercent: number | null; windowMinutes?: number | null; resetsAt?: number | null; }
export interface QuotaCredits { hasCredits?: boolean | null; unlimited?: boolean | null; balance?: string | null; }
export type QuotaStatus = "available" | "unavailable" | "disconnected" | "stale" | "parse_error" | "connected" | "unbound" | "error";
export interface SubscriptionInsight { subscriptionPrice: number | null; apiValue: number | null; roiPercent: number | null; projectedExhaustionAt: number | null; estimated: boolean; currency: string; }
export interface QuotaAccount { provider: string; name: string; plan: string; status: QuotaStatus; consuming: boolean; windows: QuotaWindow[]; credits?: QuotaCredits | null; observedAt?: string | null; source?: string | null; detail?: string | null; insight?: SubscriptionInsight | null; }
export const defaultSettings: AppSettings = { widgetVisible: true, widgetAlwaysOnTop: true, widgetCompact: false, widgetX: null, widgetY: null, widgetMetric: "tokens", widgetCollapsed: false, widgetEdge: "right", pinnedQuotaProviders: ["codex","kimi"], providerAliases: {}, planOverrides: {}, accent: "mint", density: "comfortable", includeProjectNames: true, showCostInTray: true, showTokensInTray: false, dailyReportEnabled: false, reportEmail: "", reportSmtpHost: "", reportSmtpPort: 465, reportSmtpPassword: "", reportSendTime: "08:00", reportLastSentAt: null, reportLastError: null };
