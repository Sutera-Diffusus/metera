# Metera

简体中文 · [English](README.md)

> 一款本地优先的 Windows AI 编程工具使用仪表盘。

Metera 是一款 Tauri 桌面应用，把多个 AI 编程工具的使用量、Token、成本、
配额和活动记录汇总到一个仪表盘中。它还提供亚克力悬浮仪表、Windows 托盘控制、
本地 SQLite 历史记录、可审计的供应商价格，以及可选的每日邮件报告。

![Metera 宣传图](docs/assets/metera-launch-banner.png)

当前面向用户的正式版本是 **1.7.0.1**。由于 Cargo、npm 和 Tauri 内部使用三段式
Semantic Versioning，项目包清单中的构建版本保持为 `1.7.0`。

## 功能

- 汇总使用量、成本、Token、活动和供应商数据
- 亚克力悬浮仪表与 Windows 托盘控制
- 本地 SQLite 历史记录和可配置的数据目录
- 供应商价格与成本估算
- 在本地凭据可用时显示受支持供应商的配额
- 通过用户自己的 SMTP 服务器发送每日邮件报告
- 不需要 Metera 账号、托管后端，也不包含通用使用数据遥测

## 支持的数据来源

Metera 当前可以读取或连接以下工具的本地数据或 API：

- Codex
- Claude Code
- Kimi Code
- WorkBuddy
- ZCode
- Reasonix
- DeepSeek Harness（DSH）

供应商文件格式、配额 API、认证格式和价格都可能发生变化。项目中出现的供应商名称或
Logo 不代表任何官方背书、赞助或隶属关系。

## 隐私与网络行为

Metera 是本地优先应用，但并非完全离线：

- 使用历史存储在本地 SQLite 中。
- 应用会读取本地使用记录文件，以及 Codex `auth.json`、Kimi 凭据或 DSH 凭据等本地认证文件。
- 配额和汇率功能会向对应供应商服务及配置的汇率服务发起网络请求。
- 每日报告会直接连接用户配置的 SMTP 服务器。
- Metera 不运行接收使用历史的服务器，也不包含通用分析或遥测服务。

项目名称、模型名称、Token 数量和邮件报告内容可能包含敏感信息。反馈问题时，
请不要分享 `settings.json`、本地数据库、凭据文件或未经清理的截图。

## 安装

从 [GitHub Releases](../../releases) 下载 Windows x64 安装包。安装器默认使用当前用户安装；
如果系统尚未安装 WebView2，Tauri 引导程序可能会尝试下载它。

首个公开安装包可能尚未进行代码签名，因此 Windows SmartScreen 可能显示额外警告。
安装前请核对 Release 页面中的校验值和源码。

## 数据目录

启动前设置 `METERA_DATA_DIR`，可以指定运行时数据目录：

```powershell
$env:METERA_DATA_DIR = "D:\MeteraData-test"
```

为了兼容已有安装，如果存在 `D:\MeteraData`，应用会优先使用该目录。在全新机器上，
Metera 会回退到 Windows 平台提供的标准用户数据目录；全新安装不要求存在 D: 盘。

## 开发

当前桌面应用只支持 Windows。请安装 Node.js、pnpm、Rust 1.88.0、Tauri 2 所需的
Windows 构建工具，以及运行桌面应用所需的 WebView2。

```powershell
pnpm install --frozen-lockfile
pnpm test
pnpm build
pnpm tauri dev
```

创建 Windows 安装包：

```powershell
pnpm tauri build
```

NSIS 安装包会写入 `target\release\bundle\nsis`。不要提交该目录；请将经过验证的
安装包作为 GitHub Release 附件上传。

## 已知限制

- 桌面应用当前只支持 Windows。
- 供应商集成依赖本地文件布局和供应商 API，相关内容可能变化。
- 只有在对应本地认证状态有效且可访问时，才能读取配额数据。
- SMTP 授权信息目前存储在本地设置文件中。建议使用应用专用密码并保护本地用户配置目录。
- 在完成代码签名之前，Release 安装包可能会被 Windows SmartScreen 标记为未知发布者。

## 参与贡献

请参阅 [CONTRIBUTING.md](CONTRIBUTING.md)，其中包含本地开发、验证命令和 Pull Request 要求。
请不要把真实凭据、使用数据库或包含个人信息的诊断输出提交到仓库。

## 许可证

Metera 使用 [MIT License](LICENSE) 发布。第三方依赖、供应商名称和供应商 Logo 仍受其各自条款约束。
