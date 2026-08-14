import { useEffect, useRef, useState, type ReactNode } from "react";
import { RefreshCw } from "lucide-react";
import { api } from "../../lib/api";
import { useMetera } from "../../state/MeteraContext";

function Toggle({ label, detail, checked, onChange }: { label: string; detail: string; checked: boolean; onChange(value: boolean): void }) {
  return <label className="v2-setting-row"><span><strong>{label}</strong><small>{detail}</small></span><input type="checkbox" checked={checked} onChange={event => onChange(event.target.checked)}/><i aria-hidden="true"/></label>;
}

export function SettingsView() {
  const state = useMetera();
  const [launchAtLogin, setLaunchAtLogin] = useState(false);
  useEffect(() => { void api.getLaunchAtLogin().then(setLaunchAtLogin).catch(() => undefined); }, []);
  const update = (patch: Parameters<typeof state.updateSettings>[0]) => void state.updateSettings(patch);
  const updatedAt = state.scan.lastScanAt
    ? new Date(state.scan.lastScanAt).toLocaleTimeString("zh-CN", { hour: "2-digit", minute: "2-digit", hour12: false })
    : null;
  return <div className="v2-view settings-view">
    <header className="apple-view-header">
      <h1>设置</h1>
      <div className="apple-view-header-aside">
        {updatedAt && <span>更新于 {updatedAt}</span>}
        <button className="apple-header-refresh" onClick={() => void api.triggerScan()} disabled={state.scan.status === "scanning"}>
          <RefreshCw className={state.scan.status === "scanning" ? "spin" : ""}/>{state.scan.status === "scanning" ? "同步中" : "更新数据"}
        </button>
      </div>
    </header>
    <div className="v2-settings-grid">
      <section className="v2-panel"><header className="v2-panel-header"><div><h2>浮窗</h2></div></header><div className="v2-setting-list"><Toggle label="显示浮窗" detail="关闭后可从托盘重新打开" checked={state.settings.widgetVisible} onChange={value => update({ widgetVisible: value })}/><Toggle label="紧凑浮窗" detail="只保留状态灯与主要数值" checked={state.settings.widgetCompact} onChange={value => { update({ widgetCompact: value }); void api.setWidgetCompact(value); }}/><Toggle label="始终置顶" detail="让浮窗保持在其他窗口上方" checked={state.settings.widgetAlwaysOnTop} onChange={value => update({ widgetAlwaysOnTop: value })}/><Toggle label="开机启动" detail="登录 Windows 后自动运行 Metera" checked={launchAtLogin} onChange={value => { setLaunchAtLogin(value); void api.setLaunchAtLogin(value); }}/></div></section>
      <section className="v2-panel"><header className="v2-panel-header"><div><h2>隐私与显示</h2></div></header><div className="v2-setting-list"><Toggle label="显示项目名称" detail="关闭后仪表盘不显示本地项目名" checked={state.settings.includeProjectNames} onChange={value => update({ includeProjectNames: value })}/><Toggle label="托盘显示费用" detail="在托盘提示中包含今日预估费用" checked={state.settings.showCostInTray} onChange={value => update({ showCostInTray: value })}/><Toggle label="托盘显示 Token" detail="在托盘提示中包含今日 Token" checked={state.settings.showTokensInTray} onChange={value => update({ showTokensInTray: value })}/></div></section>
    </div>
    <ScanSettingsPanel />
    <MailReportPanel/>
    <section className="v2-panel v2-data-location"><header className="v2-panel-header"><div><h2>本地数据</h2></div></header><dl><div><dt>数据目录</dt><dd>{state.status?.dataDir ?? "读取中"}</dd></div><div><dt>已识别来源</dt><dd>{state.status?.sources.join(" · ") || "暂无"}</dd></div><div><dt>应用版本</dt><dd>{state.status?.version ?? "-"}</dd></div></dl></section>
  </div>;
}

function ScanSettingsPanel() {
  const state = useMetera();
  const statusText = state.scan.status === "scanning" ? "同步中" : state.scan.status === "error" ? "同步异常" : state.scan.lastScanAt ? "已同步" : "尚未同步";
  const updatedAt = state.scan.lastScanAt ? new Date(state.scan.lastScanAt).toLocaleString("zh-CN", { month: "numeric", day: "numeric", hour: "2-digit", minute: "2-digit" }) : "暂无记录";
  return <section className="v2-panel v2-scan-panel">
    <header className="v2-panel-header"><div><h2>数据同步</h2><p>管理本地数据源扫描状态，扫描不会改变原始记录。</p></div><span className={`v2-scan-state ${state.scan.status}`}>{statusText}</span></header>
    <div className="v2-setting-list">
      <div className="v2-setting-row v2-setting-field"><span><strong>上次同步</strong><small>{updatedAt}</small></span><button className="v2-action" onClick={() => void state.refresh()} disabled={state.scan.status === "scanning"}>{state.scan.status === "scanning" ? "扫描中" : "立即同步"}</button></div>
      <div className="v2-setting-row v2-setting-field"><span><strong>已识别数据源</strong><small>{state.status?.sources.length ? state.status.sources.join(" · ") : "暂无可用数据源"}</small></span><span className="v2-scan-source-count">{state.status?.sources.length ?? 0} 个</span></div>
    </div>
  </section>;
}

const MAIL_PRESETS: Record<string, { label: string; host: string; port: number }> = {
  qq: { label: "QQ 邮箱", host: "smtp.qq.com", port: 465 },
  "163": { label: "163 邮箱", host: "smtp.163.com", port: 465 },
  gmail: { label: "Gmail", host: "smtp.gmail.com", port: 465 },
  custom: { label: "自定义", host: "", port: 465 },
};
const presetKeyFor = (host: string) => host.includes("smtp.qq.com") ? "qq" : host.includes("smtp.163.com") ? "163" : host.includes("smtp.gmail.com") ? "gmail" : "custom";

function Field({ label, detail, children }: { label: string; detail?: string; children: ReactNode }) {
  return <div className="v2-setting-row v2-setting-field"><span><strong>{label}</strong>{detail ? <small>{detail}</small> : null}</span>{children}</div>;
}

function MailReportPanel() {
  const state = useMetera();
  const fromSettings = () => ({ enabled: state.settings.dailyReportEnabled, email: state.settings.reportEmail, host: state.settings.reportSmtpHost, port: state.settings.reportSmtpPort, password: state.settings.reportSmtpPassword });
  const [draft, setDraft] = useState(fromSettings);
  const dirtyRef = useRef(false);
  const [preset, setPreset] = useState(() => presetKeyFor(state.settings.reportSmtpHost));
  const [status, setStatus] = useState<{ kind: "idle" | "sending" | "ok" | "error"; message?: string }>({ kind: "idle" });
  const [showHelp, setShowHelp] = useState(false);
  const [showLaunchGuide, setShowLaunchGuide] = useState(false);
  useEffect(() => { if (dirtyRef.current) return; const next = fromSettings(); setDraft(next); setPreset(presetKeyFor(next.host)); }, [state.settings]);
  const edit = (patch: Partial<typeof draft>) => { setDraft(current => ({ ...current, ...patch })); dirtyRef.current = true; };
  const choosePreset = (key: string) => { setPreset(key); const item = MAIL_PRESETS[key]; if (item && key !== "custom") edit({ host: item.host, port: item.port }); };
  const incomplete = !draft.email.trim() || !draft.host.trim() || !draft.password;
  const busy = status.kind === "sending";
  const save = () => {
    if (incomplete) { setStatus({ kind: "error", message: "请先填写邮箱、SMTP 服务器与授权码" }); return; }
    setStatus({ kind: "sending", message: "保存中…" });
    void api.setEmailSettings({ enabled: draft.enabled, email: draft.email.trim(), smtpHost: draft.host.trim(), smtpPort: draft.port, smtpPassword: draft.password })
      .then(() => { setStatus({ kind: "ok", message: "已保存" }); dirtyRef.current = false; })
      .catch(error => setStatus({ kind: "error", message: `保存失败: ${String(error)}` }));
  };
  const test = () => {
    if (incomplete) { setStatus({ kind: "error", message: "请先填写邮箱、SMTP 服务器与授权码" }); return; }
    setStatus({ kind: "sending", message: "发送中…" });
    void api.sendTestEmail({ email: draft.email.trim(), smtpHost: draft.host.trim(), smtpPort: draft.port, smtpPassword: draft.password })
      .then(message => setStatus({ kind: "ok", message }))
      .catch(error => setStatus({ kind: "error", message: String(error) }));
  };
  const sendNow = () => {
    setStatus({ kind: "sending", message: "报告发送中…" });
    void api.sendReportNow()
      .then(message => setStatus({ kind: "ok", message }))
      .catch(error => setStatus({ kind: "error", message: String(error) }));
  };
  const lastSentText = () => {
    if (state.settings.reportLastError) return `✗ ${state.settings.reportLastError}`;
    if (state.settings.reportLastSentAt) {
      const at = new Date(state.settings.reportLastSentAt);
      if (!Number.isNaN(at.getTime())) {
        const pad = (value: number) => String(value).padStart(2, "0");
        return `✓ ${at.getFullYear()}-${pad(at.getMonth() + 1)}-${pad(at.getDate())} ${pad(at.getHours())}:${pad(at.getMinutes())}`;
      }
    }
    return "尚未发送";
  };
  return <section className="v2-panel">
    <header className="v2-panel-header"><div><h2>每日邮件报告</h2></div></header>
    <div className="v2-setting-list">
      <Toggle label="启用每日邮件报告" detail="总开关，保存配置后生效" checked={draft.enabled} onChange={value => { edit({ enabled: value }); setShowLaunchGuide(false); if (value) void api.getLaunchAtLogin().then(on => { if (!on) setShowLaunchGuide(true); }).catch(() => undefined); }}/>
      {showLaunchGuide && draft.enabled && <div className="v2-setting-row v2-setting-field"><span><strong>开机自启未开启</strong><small>电脑重启后 Metera 没运行就无法发送当日报告，建议开启</small></span><button className="v2-action" onClick={() => void api.setLaunchAtLogin(true).then(() => setShowLaunchGuide(false)).catch(() => undefined)}>一键开启</button></div>}
      <Field label="收件邮箱" detail="同时作为 SMTP 登录账号，给自己发送"><input className="v2-setting-input" placeholder="you@qq.com" value={draft.email} onChange={event => edit({ email: event.target.value })}/></Field>
      <Field label="邮箱服务商" detail="选择预设自动填充 SMTP 参数">
        <select className="v2-setting-input" value={preset} onChange={event => choosePreset(event.target.value)}>{Object.entries(MAIL_PRESETS).map(([key, item]) => <option key={key} value={key}>{item.label}</option>)}</select>
      </Field>
      {preset === "custom" && <>
        <Field label="SMTP 服务器"><input className="v2-setting-input" placeholder="smtp.example.com" value={draft.host} onChange={event => edit({ host: event.target.value })}/></Field>
        <Field label="SMTP 端口" detail="465 为隐式 TLS，587 为 STARTTLS"><input className="v2-setting-input" type="number" placeholder="465" value={String(draft.port)} onChange={event => edit({ port: Number(event.target.value) || 465 })}/></Field>
      </>}
      <Field label="SMTP 授权码" detail="16 位授权码，不是登录密码">
        <span className="v2-setting-input-group">
          <input className="v2-setting-input" type="password" placeholder="授权码" value={draft.password} onChange={event => edit({ password: event.target.value })}/>
          <button className="v2-mail-help-toggle" onClick={() => setShowHelp(value => !value)}>{showHelp ? "收起" : "如何获取授权码？"}</button>
        </span>
      </Field>
      {showHelp && <div className="v2-mail-help">以 QQ 邮箱为例：登录 mail.qq.com → 设置 → 账户 → 开启「POP3/IMAP/SMTP 服务」→ 按提示用手机发送短信验证 → 生成 16 位授权码，粘贴到上方输入框。163 邮箱流程类似（设置 → POP3/SMTP/IMAP → 开启并获取授权码）。Gmail 需开启两步验证后创建应用专用密码。授权码不等于登录密码。</div>}
      <Field label="发送时刻" detail="每天不早于此刻发送昨日报告；00:00 = 打开即送">
        <input className="v2-setting-input" type="time" value={state.settings.reportSendTime} onChange={event => { if (event.target.value) void state.updateSettings({ reportSendTime: event.target.value }); }}/>
      </Field>
      <div className="v2-setting-row v2-setting-field">
        <span><strong>上次发送</strong><small>{lastSentText()}（立即发送使用已保存的配置）</small></span>
        <button className="v2-action" disabled={busy} onClick={sendNow}>{busy && status.message === "报告发送中…" ? "发送中…" : "立即发送昨日报告"}</button>
      </div>
      <div className="v2-setting-row v2-setting-field v2-setting-actions">
        <span className={status.kind === "error" ? "v2-mail-status v2-mail-status-error" : status.kind === "ok" ? "v2-mail-status v2-mail-status-ok" : "v2-mail-status"}>{status.message ?? ""}</span>
        <span className="v2-setting-buttons">
          <button className="v2-action" disabled={busy} onClick={test}>{busy && status.message === "发送中…" ? "发送中…" : "发送测试邮件"}</button>
          <button className="v2-action" disabled={busy} onClick={save}>保存配置</button>
        </span>
      </div>
    </div>
  </section>;
}
