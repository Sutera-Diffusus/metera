import { useEffect, useMemo, useState, type CSSProperties } from "react";
import { AnimatePresence, motion } from "motion/react";
import { Gauge, WalletCards } from "lucide-react";
import type { QuotaAccount, QuotaWindow } from "../../lib/types";
import { displaySource } from "../../lib/analytics";
import { balancePercent, balanceTone, formatBalance } from "../../lib/quota";
import { BrandIcon } from "../BrandIcon";
import { providerBrand, sourceFill } from "../brands";

const normalizeProvider = (value: string) => {
  const key = value.toLowerCase();
  if (key.includes("kimi") || key.includes("moonshot")) return "kimi-code";
  if (key.includes("codex") || key.includes("openai") || key.includes("chatgpt")) return "codex";
  if (key.includes("claude") || key.includes("anthropic")) return "claude-code";
  if (key.includes("workbuddy") || key.includes("codebuddy")) return "workbuddy";
  if (key.includes("zcode")) return "zhipu";
  if (key.includes("deepseek")) return "dsh";
  if (key.includes("reasonix")) return "reasonix";
  if (key.includes("glm") || key.includes("zhipu") || key.includes("z.ai")) return "zhipu";
  return key.replace(/\s+/g, "-");
};

const formatReset = (value: number | null | undefined) => {
  if (!value) return "--";
  const date = new Date(value);
  return `${date.toLocaleDateString("zh-CN", { weekday: "short" })} ${date.toLocaleTimeString("zh-CN", { hour: "2-digit", minute: "2-digit", hour12: false })}`;
};

const isWeekly = (window: QuotaWindow) => /7|周|week/i.test(window.label) || (window.windowMinutes ?? 0) >= 60 * 24 * 5;
const weeklyWindow = (account: QuotaAccount) => account.windows.find(isWeekly);
const shortWindow = (account: QuotaAccount) => account.windows.find(window => !isWeekly(window) && window.remainingPercent != null);
const remaining = (window: QuotaWindow | undefined) => window?.remainingPercent == null ? null : Math.max(0, Math.min(100, window.remainingPercent));
const isUsableQuota = (account: QuotaAccount) => account.status === "available" || account.status === "connected";
const quotaRemaining = (account: QuotaAccount, window: QuotaWindow | undefined) => isUsableQuota(account) ? remaining(window) : null;
const toneOf = (value: number | null) => value == null ? "neutral" : value < 20 ? "danger" : value < 50 ? "warn" : "ok";
const toneLabel = (tone: string) => tone === "danger" ? "高风险" : tone === "warn" ? "需要留意" : tone === "ok" ? "健康" : "暂无窗口";

const accountStateLabel = (account: QuotaAccount) => {
  if (account.status === "unavailable" || account.status === "disconnected" || account.status === "unbound") return "未检测到额度";
  if (account.status === "stale") return "额度信息已过期";
  if (account.status === "parse_error" || account.status === "error") return "额度读取失败";
  return account.plan || "已连接";
};

const unavailableCopy = (account: QuotaAccount) => {
  if (account.status === "stale") return `${displaySource(account.provider)} 的额度信息已过期，暂不判断为健康。`;
  if (account.status === "parse_error" || account.status === "error") return `${displaySource(account.provider)} 当前额度读取失败，暂不判断为健康。`;
  if (account.status === "unavailable" || account.status === "disconnected" || account.status === "unbound") return `${displaySource(account.provider)} 当前未检测到可用额度，不显示健康状态。`;
  return `${displaySource(account.provider)} 当前没有可用额度窗口，暂不判断为健康。`;
};

function connectedAccounts(quotas: QuotaAccount[]) {
  const map = new Map<string, QuotaAccount>();
  for (const account of quotas) map.set(normalizeProvider(account.provider), account);
  return [...map.values()]
    .filter(account => account.windows.length > 0 || account.insight?.subscriptionPrice != null)
    .sort((left, right) => {
      const leftRemaining = Math.min(...left.windows.map(window => quotaRemaining(left, window) ?? 101), 101);
      const rightRemaining = Math.min(...right.windows.map(window => quotaRemaining(right, window) ?? 101), 101);
      return leftRemaining - rightRemaining;
    })
    .slice(0, 4);
}

const accountLabel = (account: QuotaAccount) => {
  const key = `${account.provider} ${account.name}`.toLowerCase();
  return /zcode|glm|zhipu|z\.ai/.test(key) ? "智谱" : account.name || displaySource(account.provider);
};

function SegmentedQuota({ value, tone, segments = 14 }: { value: number | null; tone: string; segments?: number }) {
  const filled = value == null ? 0 : Math.max(1, Math.round(value / 100 * segments));
  return <div className={`overview-v5-segments ${tone}`} aria-label={value == null ? "额度未知" : `额度剩余 ${Math.round(value)}%`}>
    {Array.from({ length: segments }, (_, index) => <motion.i key={index} className={index < filled ? "filled" : ""} initial={false} animate={{ opacity: index < filled ? 1 : .42, scaleX: index < filled ? 1 : .94 }} transition={{ duration: .22, delay: index * .012 }}/>) }
  </div>;
}

function projectedHorizon(projected: number | null | undefined) {
  if (!projected) return { label: "--", hours: null as number | null };
  const hours = (projected - Date.now()) / 3_600_000;
  if (hours <= 0) return { label: "已耗尽", hours: 0 };
  if (hours < 24) return { label: `约 ${Math.max(1, Math.round(hours))}h`, hours };
  return { label: `约 ${Math.max(1, Math.round(hours / 24))}d`, hours };
}

/// 首页「套餐与账户」账户清单：跟随浮窗固定（pinnedQuotaProviders，最多 2 个）；
/// 未固定任何账户时回退到有额度窗口的账户（兼容旧行为，避免首页空栏）。
export function pinnedBalanceAccounts(quotas: QuotaAccount[], pinned?: string[]): QuotaAccount[] {
  if (pinned?.length) {
    const map = new Map(quotas.map(account => [account.provider, account]));
    const list = pinned
      .map(provider => map.get(provider))
      .filter((account): account is QuotaAccount => Boolean(account))
      .slice(0, 2);
    if (list.length) return list;
  }
  return connectedAccounts(quotas);
}

export function OverviewQuotaBalance({ quotas, pinned, onInspect }: { quotas: QuotaAccount[]; pinned?: string[]; onInspect?(): void }) {
  const accounts = useMemo(() => pinnedBalanceAccounts(quotas, pinned), [quotas, pinned]);
  return <section className="overview-v4-panel overview-v5-balance-panel">
    <header className="overview-v4-panel-header">
      <div className="overview-v5-heading-with-icon"><span className="overview-v5-panel-symbol accounts"><WalletCards/></span><div><p>SUBSCRIPTIONS</p><h2>套餐与账户</h2></div></div>
      {onInspect && <button className="overview-v4-panel-link" onClick={onInspect}>查看全部 →</button>}
    </header>
    <div className="overview-v5-account-list">
      {accounts.map((account, index) => {
        const week = weeklyWindow(account) ?? account.windows[0];
        const value = quotaRemaining(account, week);
        const tone = toneOf(value);
        // 余额型账户（无周期窗口、有余额）：主数值显示余额，进度条按余额百分比（100 元 = 100%）填充。
        const balanceOnly = account.windows.length === 0 ? account.credits?.balance : null;
        const balancePct = balanceOnly ? balancePercent(balanceOnly) : null;
        const balanceToneValue = balanceOnly ? balanceTone(balancePct) : null;
        return <motion.div className="overview-v5-account-row" key={account.provider} initial={{ opacity: 0, y: 8, filter: "blur(3px)" }} animate={{ opacity: 1, y: 0, filter: "blur(0px)" }} whileHover={{ y: -2 }} transition={{ duration: .22, delay: index * .045, ease: [0.16, 1, 0.3, 1] }}>
          <span className="overview-v4-brand-box" style={{ "--brand-color": sourceFill(normalizeProvider(account.provider)) } as CSSProperties}><BrandIcon brand={providerBrand(account.provider)} size={18}/></span>
          <div className="overview-v5-account-name"><strong>{accountLabel(account)}</strong><span>{balanceOnly ? "余额" : accountStateLabel(account)}</span></div>
          <div className="overview-v5-account-quota">{balanceOnly ? <><b className={`num ${balanceToneValue}`}>{formatBalance(balanceOnly)}</b><SegmentedQuota value={balancePct} tone={balanceToneValue ?? "neutral"} segments={12}/></> : <><b className={tone}>{value == null ? "额度未知" : `剩余 ${Math.round(value)}%`}</b><SegmentedQuota value={value} tone={tone} segments={12}/></>}</div>
          <span className="overview-v5-account-reset num">{balanceOnly ? "实时" : value == null ? "--" : formatReset(week?.resetsAt)}</span>
        </motion.div>;
      })}
      {!accounts.length && <div className="overview-v4-empty-table">暂无可读套餐余额</div>}
    </div>
  </section>;
}

export function OverviewQuotaRisk({ quotas, onInspect }: { quotas: QuotaAccount[]; onInspect?(): void }) {
  const accounts = useMemo(() => connectedAccounts(quotas), [quotas]);
  const riskAccounts = useMemo(() => [...accounts]
    .filter(account => account.windows.some(window => quotaRemaining(account, window) != null))
    .sort((left, right) => Math.min(...left.windows.map(window => quotaRemaining(left, window) ?? 101), 101) - Math.min(...right.windows.map(window => quotaRemaining(right, window) ?? 101), 101)), [accounts]);
  const [activeIndex, setActiveIndex] = useState(0);
  useEffect(() => { setActiveIndex(0); }, [riskAccounts.map(account => `${account.provider}:${account.observedAt ?? ""}`).join("|")]);

  if (!riskAccounts.length) {
    return <section className="overview-v4-panel overview-v5-risk-panel">
      <header className="overview-v4-panel-header"><div className="overview-v5-heading-with-icon"><span className="overview-v5-panel-symbol quota"><Gauge/></span><div><p>QUOTA STATUS</p><h2>额度状态</h2></div></div></header>
      <div className="overview-v5-risk-empty"><span>暂无可读额度窗口</span>{onInspect && <button className="overview-v4-panel-link" onClick={onInspect}>前往账户详情</button>}</div>
    </section>;
  }

  const account = riskAccounts[activeIndex] ?? riskAccounts[0];
  const week = weeklyWindow(account) ?? account.windows[0];
  const short = shortWindow(account);
  const weekRemaining = quotaRemaining(account, week);
  const shortRemaining = quotaRemaining(account, short);
  const candidates = [weekRemaining, shortRemaining].filter((value): value is number => value != null);
  const riskValue = candidates.length ? Math.min(...candidates) : null;
  const tone = toneOf(riskValue);
  const next = riskAccounts[(activeIndex + 1) % riskAccounts.length];
  const projected = account.insight?.projectedExhaustionAt;
  const horizon = projectedHorizon(projected);
  const used = riskValue == null ? null : Math.max(0, 100 - riskValue);
  const copy = !isUsableQuota(account) || riskValue == null
    ? unavailableCopy(account)
    : tone === "danger"
      ? `${displaySource(account.provider)} 的额度消耗速度较快，建议降低高成本模型调用，或暂时切换到其他来源。`
      : tone === "warn"
        ? `${displaySource(account.provider)} 仍可使用，但需要留意窗口重置前的剩余额度。`
        : `${displaySource(account.provider)} 当前额度健康，可以继续按现有节奏使用。`;
  const pathEndY = riskValue == null ? 30 : Math.max(8, Math.min(44, 46 - riskValue * .36));

  return <section className={`overview-v4-panel overview-v5-risk-panel ${tone}`}>
    <header className="overview-v4-panel-header">
      <div className="overview-v5-heading-with-icon"><span className="overview-v5-panel-symbol quota"><Gauge/></span><div><p>QUOTA STATUS</p><h2>额度状态</h2></div></div>
      <span className={`overview-v4-risk-badge ${tone}`}>{toneLabel(tone)}</span>
    </header>
    <AnimatePresence mode="wait" initial={false}>
    <motion.div className="overview-v5-risk-body" key={`${account.provider}-${account.observedAt ?? ""}`} initial={{ opacity: 0, x: 8, filter: "blur(4px)" }} animate={{ opacity: 1, x: 0, filter: "blur(0px)" }} exit={{ opacity: 0, x: -6, filter: "blur(4px)" }} transition={{ duration: .24, ease: [0.16, 1, 0.3, 1] }}>
      <div className="overview-v5-risk-account"><span className="overview-v4-brand-box"><BrandIcon brand={providerBrand(account.provider)} size={21}/></span><div><strong>{accountLabel(account)}</strong><span>{accountStateLabel(account)}</span></div></div>
      <div className="overview-v5-risk-value"><strong className="num">{riskValue == null ? "--" : Math.round(riskValue)}</strong>{riskValue != null && <b>%</b>}<span>{week?.label ?? "额度窗口"}剩余</span></div>
      <SegmentedQuota value={riskValue} tone={tone}/>
      <div className="overview-v5-risk-meta"><div><span>预计耗尽</span><strong>{projected ? formatReset(projected) : "暂无可靠预测"}</strong></div><div><span>下次重置</span><strong>{formatReset(week?.resetsAt ?? short?.resetsAt)}</strong></div></div>
      <div className="overview-v5-risk-insights">
        <div><span>窗口已用</span><b className={tone}>{used == null ? "--" : `${Math.round(used)}%`}</b></div>
        <div><span>{short?.label ?? "短窗剩余"}</span><b className={toneOf(shortRemaining)}>{shortRemaining == null ? "--" : `${Math.round(shortRemaining)}%`}</b></div>
        <div><span>预计可用</span><b className="projection">{horizon.label}</b></div>
      </div>
      <div className="overview-v5-risk-projection">
        <div><span>消耗预测</span><small>{projected ? "基于近期节奏" : "等待更多观察数据"}</small></div>
        <svg viewBox="0 0 260 54" preserveAspectRatio="none" aria-hidden="true"><path className="guide" d="M4 44 H256"/>{projected && <><motion.path className={tone} d={`M4 12 C72 14 128 18 176 25 S226 ${pathEndY} 256 ${pathEndY}`} initial={{ pathLength: 0 }} animate={{ pathLength: 1 }} transition={{ duration: .65, ease: [0.22, 1, .36, 1] }}/><circle className={tone} cx="256" cy={pathEndY} r="3.2"/></>}</svg>
      </div>
      <p className="overview-v5-risk-copy">{copy}</p>
      <div className="overview-v5-risk-footer">{riskAccounts.length > 1 ? <button className="overview-v4-next-provider" onClick={() => setActiveIndex(index => (index + 1) % riskAccounts.length)} aria-label={`切换到 ${displaySource(next.provider)}`}>下一项　▶ {displaySource(next.provider)}</button> : onInspect && <button className="overview-v4-panel-link" onClick={onInspect}>账户详情</button>}</div>
    </motion.div>
    </AnimatePresence>
  </section>;
}
