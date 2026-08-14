import { Check, ExternalLink, RefreshCw } from "lucide-react";
import { AnimatePresence, motion } from "motion/react";
import { useEffect, useMemo, useState } from "react";
import { RollingNumber } from "../../components/RollingNumber";
import { api } from "../../lib/api";
import { formatTokens } from "../../lib/analytics";
import type { QuotaAccount, QuotaWindow, UsageBucket } from "../../lib/types";
import { plansForProvider } from "../../lib/pricing";
import { useMetera } from "../../state/MeteraContext";
import { BrandIcon } from "../BrandIcon";
import { bucketTokens } from "../dashboardAnalytics";
import { providerBrand } from "../brands";

const statusLabel = (status: QuotaAccount["status"]) => status === "available" || status === "connected" ? "额度可用" : status === "unavailable" ? "等待额度" : status === "stale" ? "数据过期" : status === "parse_error" || status === "error" ? "读取失败" : "未连接";

const toneOf = (remaining: number | null) => remaining == null ? "unknown" : remaining > 50 ? "good" : remaining >= 20 ? "warn" : "danger";
const TONE_COLORS: Record<string, string> = { good: "var(--ok)", warn: "var(--warn)", danger: "var(--danger)", unknown: "var(--v2-line-strong)" };

const findWindow = (account: QuotaAccount, kind: "5h" | "weekly") =>
  kind === "5h"
    ? account.windows.find(window => window.windowMinutes === 300) ?? account.windows.find(window => /5|五/.test(window.label))
    : account.windows.find(window => window.windowMinutes === 10080) ?? account.windows.find(window => /周|7\s*天|week/i.test(window.label));

// §17.6 额度与账号：双环 gauge（5h 内环 + 周窗外环）+ 回本镜像龙卷风条。
export function AccountsView() {
  const state = useMetera();
  const [message, setMessage] = useState<string | null>(null);
  const [habitBuckets, setHabitBuckets] = useState<UsageBucket[]>([]);
  useEffect(() => {
    let mounted = true;
    const end = new Date();
    const start = new Date(end);
    start.setDate(start.getDate() - 29);
    start.setHours(0, 0, 0, 0);
    void api.usage(start.toISOString(), end.toISOString()).then(result => { if (mounted) setHabitBuckets(result.buckets); }).catch(() => { if (mounted) setHabitBuckets([]); });
    return () => { mounted = false; };
  }, [state.chartReplay]);
  const bind = async (account: QuotaAccount) => {
    setMessage(null);
    try { await api.bindAccount(account.provider); setMessage(`${account.name} 的官方登录窗口已打开`); }
    catch (reason) { setMessage(String(reason)); }
  };
  const togglePin = (provider: string) => {
    const current = state.settings.pinnedQuotaProviders;
    const next = current.includes(provider) ? current.filter(value => value !== provider) : [...current, provider].slice(-2);
    void state.updateSettings({ pinnedQuotaProviders: next });
  };
  const updatedAt = state.scan.lastScanAt
    ? new Date(state.scan.lastScanAt).toLocaleTimeString("zh-CN", { hour: "2-digit", minute: "2-digit", hour12: false })
    : null;
  return <div className="v2-view v2-accounts-view">
    <header className="apple-view-header">
      <h1>额度与账号</h1>
      <div className="apple-view-header-aside">
        {updatedAt && <span>更新于 {updatedAt}</span>}
        <button className="apple-header-refresh" onClick={() => void state.refresh()}><RefreshCw/>重新检测</button>
      </div>
    </header>
    <AnimatePresence initial={false}>{message && <motion.div className="v2-message" role="status" initial={{ opacity: 0, y: -6, scale: .985, filter: "blur(4px)" }} animate={{ opacity: 1, y: 0, scale: 1, filter: "blur(0px)" }} exit={{ opacity: 0, y: -4, scale: .985, filter: "blur(4px)" }} transition={{ duration: .2, ease: [0.16, 1, 0.3, 1] }}>{message}</motion.div>}</AnimatePresence>
    {state.shortageAlerts.length > 0 && <div className="v2-shortage-alerts" role="alert">{state.shortageAlerts.map((alert, index) => <div key={index}><span className="alert-dot"/>⚠ {alert}</div>)}</div>}
    <HabitCurve buckets={habitBuckets}/>
    <div className="v2-account-list">{state.quotas.map((account, index) => {
      const pinned = state.settings.pinnedQuotaProviders.includes(account.provider);
      return <motion.article layout className="v2-account-card" key={account.provider} initial={{ opacity: 0, y: 12, filter: "blur(4px)" }} animate={{ opacity: 1, y: 0, filter: "blur(0px)" }} whileHover={{ y: -3 }} transition={{ duration: .24, delay: index * .05, ease: [0.16, 1, 0.3, 1] }}>
        <div className="account-identity">
          <div className={`account-mark ${account.status}`}><BrandIcon brand={providerBrand(account.provider)} size={22}/></div>
          <div><h2>{account.name}</h2><p>{account.plan || "套餐类型暂不可用"}</p></div>
          <span className={`account-state ${account.status}`}>{statusLabel(account.status)}</span>
          <div className={`account-consuming${account.consuming ? " active" : ""}`}><i/><span>{account.consuming ? "正在使用" : "当前未消耗"}</span></div>
        </div>
        <DualRing fiveHour={findWindow(account, "5h")} weekly={findWindow(account, "weekly")} replayKey={state.chartReplay} replay={state.chartReplayActive}/>
        <InsightTornado account={account}/>
        <div className="account-detail">
          <dl><div><dt>套餐</dt><dd>{account.plan || "暂不可用"}</dd></div><div><dt>额度来源</dt><dd>{account.windows.length ? "服务实时返回" : "暂未返回"}</dd></div><div><dt>状态</dt><dd>{account.detail ?? statusLabel(account.status)}</dd></div></dl>
          <PlanSelector account={account}/>
          <footer><button className={pinned ? "active" : ""} onClick={() => togglePin(account.provider)} aria-pressed={pinned}><Check/>浮窗显示</button>{(account.status === "disconnected" || account.status === "unbound") && <button onClick={() => void bind(account)}><ExternalLink/>官方登录</button>}</footer>
        </div>
      </motion.article>;
    })}{!state.quotas.length && <div className="v2-empty">尚未检测到可绑定的额度服务</div>}</div>
  </div>;
}

/** 双环 gauge：外环 = 周额度，内环 = 5 小时额度；中心读数取更紧的一环。 */
function HabitCurve({ buckets }: { buckets: UsageBucket[] }) {
  const days = useMemo(() => {
    const now = new Date();
    const result = Array.from({ length: 30 }, (_, index) => {
      const date = new Date(now);
      date.setHours(0, 0, 0, 0);
      date.setDate(date.getDate() - (29 - index));
      return { key: date.toISOString().slice(0, 10), label: `${date.getMonth() + 1}/${date.getDate()}`, tokens: 0 };
    });
    const byDate = new Map(result.map(day => [day.key, day]));
    for (const bucket of buckets) {
      const day = byDate.get(bucket.bucketStart.slice(0, 10));
      if (day) day.tokens += bucketTokens(bucket);
    }
    return result;
  }, [buckets]);
  const plottedDays = days.slice(Math.max(0, days.findIndex(day => day.tokens > 0)));
  const peak = Math.max(1, ...plottedDays.map(day => day.tokens));
  const total = days.reduce((sum, day) => sum + day.tokens, 0);
  const points = plottedDays.map((day, index) => `${18 + index * 684 / Math.max(1, plottedDays.length - 1)},${132 - day.tokens / peak * 94}`).join(" ");
  const area = `M18 132 L${points.replace(/ /g, " L")} L702 132 Z`;
  return <section className="v2-panel account-habit-panel">
    <header className="v2-panel-header"><div><h2>使用习惯 · 近 30 天</h2><p>观察调用节奏，不把历史使用量当作供应商实时配额。</p></div><span>{total ? `${formatTokens(total)} Token` : "暂无数据"}</span></header>
    {total ? <div className="account-habit-chart"><div className="account-habit-readout"><strong className="num">{formatTokens(Math.round(total / 30))}</strong><small>日均 Token</small></div><svg viewBox="0 0 720 160" role="img" aria-label={`近 30 天使用习惯，总计 ${formatTokens(total)} Token`}><motion.path className="account-habit-area" d={area} initial={{ opacity: 0 }} animate={{ opacity: 1 }} transition={{ duration: .35, ease: [0.16, 1, 0.3, 1] }}/><motion.polyline className="account-habit-line" points={points} initial={{ pathLength: 0, opacity: 0 }} animate={{ pathLength: 1, opacity: 1 }} transition={{ duration: .72, ease: [0.16, 1, 0.3, 1] }}/>{plottedDays.map((day, index) => <motion.circle key={day.key} className="account-habit-point" cx={18 + index * 684 / Math.max(1, plottedDays.length - 1)} cy={132 - day.tokens / peak * 94} r="3" tabIndex={0} initial={{ opacity: 0, scale: 0 }} animate={{ opacity: 1, scale: 1 }} transition={{ delay: .25 + index * .018, duration: .22, type: "spring", stiffness: 460, damping: 28 }}><title>{day.label} · {formatTokens(day.tokens)} Token</title></motion.circle>)}</svg><div className="account-habit-axis"><span>{plottedDays[0]?.label}</span><span>{plottedDays.at(-1)?.label}</span></div></div> : <div className="v2-empty">近 30 天暂无可用使用数据</div>}
  </section>;
}

function DualRing({ fiveHour, weekly, replayKey, replay }: { fiveHour?: QuotaWindow; weekly?: QuotaWindow; replayKey: number; replay: boolean }) {
  const rings = [
    { key: "weekly", title: "周额度", window: weekly, r: 51 },
    { key: "5h", title: "5 小时额度", window: fiveHour, r: 37 },
  ];
  const remainingOf = (window?: QuotaWindow) => window?.remainingPercent ?? null;
  const tightest = rings.reduce<{ remaining: number | null; title: string }>((best, ring) => {
    const remaining = remainingOf(ring.window);
    return remaining != null && (best.remaining == null || remaining < best.remaining) ? { remaining, title: ring.title } : best;
  }, { remaining: null, title: "" });
  return <div className="dual-ring">
    <div className="dual-ring-visual">
      <svg viewBox="0 0 120 120" role="img" aria-label={rings.map(ring => { const r = remainingOf(ring.window); return `${ring.title} ${r == null ? "暂不可用" : `剩余 ${Math.round(r)}%`}`; }).join("，")}>
        {rings.map(ring => {
          const remaining = remainingOf(ring.window);
          const normalized = remaining == null ? 0 : Math.max(0, Math.min(100, remaining));
          return <g key={ring.key}>
            <circle className="dual-ring-track" cx="60" cy="60" r={ring.r}/>
            <motion.circle key={`${replayKey}-${ring.key}-${normalized}`} cx="60" cy="60" r={ring.r} pathLength="1"
              stroke={TONE_COLORS[toneOf(remaining)]} className="dual-ring-value"
              transform={`rotate(-90 60 60)`}
              initial={replay ? { pathLength: 0 } : false} animate={{ pathLength: Math.max(.004, normalized / 100) }}
              transition={{ duration: .78, ease: [0.16, 1, 0.3, 1] }}/>
          </g>;
        })}
      </svg>
      <strong className="num">{tightest.remaining == null ? "--" : <RollingNumber value={`${Math.round(tightest.remaining)}%`}/>}</strong>
      <small>{tightest.remaining == null ? "额度" : `${tightest.title}剩余`}</small>
    </div>
    <div className="dual-ring-legend">
      {rings.map(ring => {
        const remaining = remainingOf(ring.window);
        return <div className="dual-ring-row" key={ring.key}>
          <i style={{ background: TONE_COLORS[toneOf(remaining)] }}/><b>{ring.title}</b>
          <span className="num">{remaining == null ? "--" : `${Math.round(remaining)}%`}</span>
          <time>{ring.window?.resetsAt ? `${new Date(ring.window.resetsAt).toLocaleString([], { month: "numeric", day: "numeric", hour: "2-digit", minute: "2-digit" })} 重置` : "重置时间暂不可用"}</time>
        </div>;
      })}
    </div>
  </div>;
}

/** §16/§17 订阅洞察：用同一基线对比订阅实付与 API 折算价值，并把回本率作为结论呈现。 */
function InsightTornado({ account }: { account: QuotaAccount }) {
  const insight = account.insight;
  if (!insight) return null;
  const { subscriptionPrice, apiValue, roiPercent, projectedExhaustionAt, estimated, currency } = insight;
  const symbol = currency === "CNY" ? "¥" : "$";
  const fmt = (value: number | null) => value == null ? "--" : `${symbol}${value.toLocaleString([], { maximumFractionDigits: value >= 100 ? 0 : 2 })}`;
  const maxBar = Math.max(subscriptionPrice ?? 0, apiValue ?? 0, 1);
  const subShare = (subscriptionPrice ?? 0) / maxBar;
  const apiShare = (apiValue ?? 0) / maxBar;
  const subBar = subscriptionPrice == null ? 0 : Math.min(1, Math.max(.04, subShare));
  const apiBar = apiValue == null ? 0 : Math.min(1, Math.max(.04, apiShare));
  const roi = roiPercent == null ? null : Math.round(roiPercent);
  const roiClass = roi == null ? "" : roi >= 100 ? "roi-hit" : roi >= 50 ? "roi-mid" : "roi-low";
  const roiStatus = roi == null ? "回本率未知" : roi >= 100 ? "已回本" : "尚未回本";
  const exhaustion = projectedExhaustionAt ? new Date(projectedExhaustionAt) : null;
  return <div className={`account-insight tornado ${roiClass}`}>
    <div className="insight-head">
      <div><span className="insight-title">订阅回本对比</span><small className="insight-subtitle">订阅实付 · API 折算价值</small></div>
      {estimated ? <span className="insight-badge estimate" title="真实额度来源不可得，按订阅价 ÷ API 单价折算">估算</span> : <span className="insight-badge live" title="额度来自服务实时返回">实时</span>}
    </div>
    <div className="roi-summary">
      <div className="roi-rate"><strong className="num">{roi == null ? "--" : `${roi}%`}</strong><span>回本率</span></div>
      <div className="roi-status"><i aria-hidden="true"/><span>{roiStatus}</span></div>
    </div>
    <div className="roi-comparison" role="img" aria-label={`订阅实付 ${fmt(subscriptionPrice)}，API 折算价值 ${fmt(apiValue)}，回本率 ${roi == null ? "未知" : roi + "%"}`}>
      <div className="roi-comparison-row">
        <div className="roi-comparison-top"><span>订阅实付</span><strong className="num">{fmt(subscriptionPrice)}</strong></div>
        <div className="roi-comparison-track"><motion.i className="roi-comparison-fill subscription" initial={{ scaleX: 0 }} animate={{ scaleX: subBar }} transition={{ duration: .6, ease: [0.16, 1, 0.3, 1] }}/></div>
      </div>
      <div className="roi-comparison-row">
        <div className="roi-comparison-top"><span>API 折算</span><strong className="num">{fmt(apiValue)}</strong></div>
        <div className="roi-comparison-track"><motion.i className={`roi-comparison-fill api${roi != null && roi >= 100 ? " hit" : ""}`} initial={{ scaleX: 0 }} animate={{ scaleX: apiBar }} transition={{ duration: .6, ease: [0.16, 1, 0.3, 1] }}/></div>
      </div>
    </div>
    {exhaustion && <div className="insight-exhaustion">预计 <time>{exhaustion.toLocaleDateString([], { month: "numeric", day: "numeric" })}</time> 耗尽{estimated ? "（估算）" : ""}</div>}
  </div>;
}

/** 订阅档位手动选择器：自动检测不可靠（codex plan_type 常为 null、Kimi 依赖登录态）时，
 *  用户在这里指定实际套餐，回本对比按所选档位计算。 */
function PlanSelector({ account }: { account: QuotaAccount }) {
  const state = useMetera();
  const providerKey = account.provider === "codex" ? "chatgpt" : account.provider;
  const plans = plansForProvider(providerKey);
  const current = state.settings.planOverrides[account.provider] ?? "";
  const apply = (plan: string) => {
    const next = { ...state.settings.planOverrides };
    if (plan) next[account.provider] = plan; else delete next[account.provider];
    void state.updateSettings({ planOverrides: next });
  };
  if (!plans.length) return null;
  return <div className="plan-selector">
    <label htmlFor={`plan-${account.provider}`}>订阅档位</label>
    <select id={`plan-${account.provider}`} value={current} onChange={event => apply(event.target.value)}>
      <option value="">自动检测</option>
      {plans.filter(plan => plan.priceMonthly > 0).map(plan => (
        <option key={plan.plan} value={plan.plan}>
          {plan.plan} · {plan.currency === "CNY" ? "¥" : "$"}{plan.priceMonthly}/月
        </option>
      ))}
    </select>
  </div>;
}
