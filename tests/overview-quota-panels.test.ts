import { describe, expect, it } from "vitest";
import { pinnedBalanceAccounts } from "../src/dashboard/views/OverviewQuotaPanels";
import { balancePercent, balanceTone, formatBalance } from "../src/lib/quota";
import type { QuotaAccount, QuotaWindow } from "../src/lib/types";

const window5h = (): QuotaWindow => ({ kind: null, label: "5 小时", usedPercent: 30, remainingPercent: 70, windowMinutes: 300, resetsAt: Date.now() + 3600_000 });

const account = (provider: string, windows: QuotaWindow[] = [], balance: string | null = null): QuotaAccount => ({
  provider, name: provider, plan: "plan", status: "connected", consuming: false,
  windows, credits: balance ? { hasCredits: true, unlimited: null, balance } : null,
  observedAt: null, source: null, detail: null, insight: null,
});

describe("pinnedBalanceAccounts", () => {
  it("follows pinned providers in pin order and caps at 2", () => {
    const quotas = [account("codex", [window5h()]), account("kimi", [window5h()]), account("deepseek", [], "2.91 CNY")];
    const list = pinnedBalanceAccounts(quotas, ["deepseek", "codex", "kimi"]);
    expect(list.map(item => item.provider)).toEqual(["deepseek", "codex"]);
  });

  it("includes pinned balance-only accounts even without quota windows", () => {
    const quotas = [account("codex", [window5h()]), account("deepseek", [], "2.91 CNY")];
    const list = pinnedBalanceAccounts(quotas, ["deepseek"]);
    expect(list.map(item => item.provider)).toEqual(["deepseek"]);
  });

  it("falls back to windowed accounts when nothing is pinned", () => {
    const quotas = [account("codex", [window5h()]), account("kimi", [window5h()]), account("deepseek", [], "2.91 CNY")];
    const list = pinnedBalanceAccounts(quotas, []);
    expect(list.length).toBeGreaterThan(0);
    expect(list.map(item => item.provider)).not.toContain("deepseek");
  });

  it("skips pinned providers that are not present in quotas", () => {
    const quotas = [account("codex", [window5h()])];
    expect(pinnedBalanceAccounts(quotas, ["ghost", "codex"]).map(item => item.provider)).toEqual(["codex"]);
  });
});

describe("formatBalance", () => {
  it("formats CNY and USD balances with currency symbols", () => {
    expect(formatBalance("2.91 CNY")).toBe("¥2.91");
    expect(formatBalance("5.00 USD")).toBe("$5.00");
  });

  it("passes through unrecognized formats", () => {
    expect(formatBalance("unknown")).toBe("unknown");
  });
});

describe("balancePercent / balanceTone", () => {
  it("uses 100 yuan as the 100% baseline and caps at 100", () => {
    expect(balancePercent("100.00 CNY")).toBe(100);
    expect(balancePercent("101.40 CNY")).toBe(100);
    expect(balancePercent("30.00 CNY")).toBe(30);
    expect(balancePercent("2.91 CNY")).toBe(2.91);
    expect(balancePercent("0 CNY")).toBe(0);
  });

  it("applies USD at the same 100-unit scale", () => {
    expect(balancePercent("50.00 USD")).toBe(50);
  });

  it("returns null for unrecognized formats", () => {
    expect(balancePercent("unknown")).toBeNull();
  });

  it("turns yellow at 30 yuan and red at 10 yuan", () => {
    expect(balanceTone(balancePercent("100.00 CNY"))).toBe("ok");
    expect(balanceTone(balancePercent("30.01 CNY"))).toBe("ok");
    expect(balanceTone(balancePercent("30.00 CNY"))).toBe("warn");
    expect(balanceTone(balancePercent("15.00 CNY"))).toBe("warn");
    expect(balanceTone(balancePercent("10.00 CNY"))).toBe("danger");
    expect(balanceTone(balancePercent("3.00 CNY"))).toBe("danger");
    expect(balanceTone(null)).toBe("neutral");
  });
});
