// 余额型额度账户的显示口径（DeepSeek 等无周期窗口、按余额计量的账户）。
// 用户定案（2026-08-14）：100 元人民币 = 100%；剩余 30 元变黄（≤30）、10 元变红（≤10）；
// USD 余额按同尺度（100 美元 = 100%）处理，阈值同 30/10。

/// 余额文本（credits.balance，如 "2.91 CNY"）→ 本地化货币显示（"¥2.91" / "$5.00"）。
export const formatBalance = (balance: string) => {
  const match = balance.match(/^([\d.]+)\s*(CNY|USD)$/i);
  if (!match) return balance;
  const [, amount, currency] = match;
  return `${currency.toUpperCase() === "CNY" ? "¥" : "$"}${amount}`;
};

/// 余额 → 进度百分比（基线 100：100 元/美元 = 100%，封顶 100）。无法解析返回 null。
export const balancePercent = (balance: string): number | null => {
  const match = balance.match(/^([\d.]+)\s*(CNY|USD)$/i);
  if (!match) return null;
  const amount = Number(match[1]);
  if (!Number.isFinite(amount)) return null;
  return Math.max(0, Math.min(100, amount));
};

/// 余额档位：>30 正常(ok) / 10–30 注意(warn) / ≤10 危险(danger)；无法解析 neutral。
export const balanceTone = (percent: number | null): string =>
  percent == null ? "neutral" : percent > 30 ? "ok" : percent > 10 ? "warn" : "danger";
