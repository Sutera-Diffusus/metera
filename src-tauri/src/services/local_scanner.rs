use crate::state::{AppCtx, SyncState};
use metera_core::parsers::{claude_code::ClaudeCodeParser, codex::CodexParser, dsh::DshParser, kimi_code::KimiCodeParser, reasonix::ReasonixParser, workbuddy::WorkBuddyParser, zcode::ZcodeParser};
use tauri::{AppHandle, Emitter, Manager};

fn publish(app: &AppHandle, state: SyncState) {
    *app.state::<AppCtx>().sync_state.lock().unwrap_or_else(|e| e.into_inner()) = state.clone();
    let _ = app.emit("scan-state", state);
}

fn now_ms() -> u64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH)
        .map(|value| value.as_millis() as u64).unwrap_or(0)
}

pub async fn run(app: AppHandle) {
    let ctx = app.state::<AppCtx>();
    let Ok(_guard) = ctx.sync_running.try_lock() else { return };
    let previous_scan_at = ctx.sync_state.lock().unwrap_or_else(|e| e.into_inner()).last_scan_at;
    publish(&app, SyncState { status: "scanning".into(), message: Some("正在读取本地记录".into()), last_scan_at: previous_scan_at });

    let codex_cache = ctx.codex_cache_dir.clone();
    let home = ctx.home_dir.clone();
    // §8 路径自适应：每次扫描重新探测各数据源路径,路径迁移后无需重启即可自动识别。
    let paths = crate::state::resolve_paths(&home);
    let codex_home = paths.codex_home;
    let kimi = paths.kimi_code_dir;
    let kimi_legacy = paths.kimi_legacy_dir;
    let workbuddy = paths.workbuddy_projects_dir;
    let reasonix = paths.reasonix_projects_dir;
    let dsh = paths.dsh_sessions_dir;
    let reasonix_cache = ctx.data_dir.join("cache").join("reasonix-incremental.json");
    let include_projects = ctx.settings.lock().unwrap_or_else(|e| e.into_inner()).include_project_names;
    let hostname = AppCtx::hostname();
    let parsed = tauri::async_runtime::spawn_blocking(move || vec![
        ("Codex", "codex", CodexParser::new(codex_home, codex_cache).parse(&hostname, include_projects).map_err(|e| e.to_string())),
        ("Claude Code", "claude-code", ClaudeCodeParser::new(&home).parse(&hostname, include_projects).map_err(|e| e.to_string())),
        ("Kimi Code", "kimi-code", KimiCodeParser::new(kimi, kimi_legacy).parse(&hostname, false).map_err(|e| e.to_string())),
        ("WorkBuddy", "workbuddy", WorkBuddyParser::new(workbuddy).parse(&hostname, include_projects).map_err(|e| e.to_string())),
        ("ZCode", "zcode", ZcodeParser::new(&home).parse(&hostname, include_projects).map_err(|e| e.to_string())),
        ("Reasonix", "reasonix", ReasonixParser::with_cache(reasonix, reasonix_cache).parse(&hostname, include_projects).map_err(|e| e.to_string())),
        ("DeepSeek Harness", "dsh", DshParser::new(dsh).parse(&hostname, include_projects).map_err(|e| e.to_string())),
    ]).await;

    let mut details = Vec::new(); let mut successful_sources = 0usize;
    // §8 活性告警：连续空扫描计数（每源），达到阈值提示"路径可能已迁移"。
    static EMPTY_SCAN_COUNTS: std::sync::LazyLock<std::sync::Mutex<std::collections::HashMap<String, u32>>> = std::sync::LazyLock::new(|| std::sync::Mutex::new(std::collections::HashMap::new()));
    if let Ok(results) = parsed {
        for (source, source_id, result) in results {
            match result {
                Ok(output) => {
                    // 可观测性：每源解析规模入日志（GUI 无控制台时经 stderr 重定向捕获）。
                    log::info!(
                        "scan {source} ({source_id}): files={} buckets={} sessions={} records={} malformed={}",
                        output.files_scanned, output.buckets.len(), output.sessions.len(), output.usage_records, output.malformed_lines
                    );
                    let mut repository = ctx.usage.lock().unwrap_or_else(|e| e.into_inner());
                    // 空扫描保护：解析结果为空但库中已有该源历史数据时,跳过重建,
                    // 避免路径解析错误/文件被锁等临时故障把整源历史清空。
                    if output.buckets.is_empty() && output.sessions.is_empty() {
                        match repository.source_record_count(source_id) {
                            Ok(existing) if existing > 0 => {
                                let mut counts = EMPTY_SCAN_COUNTS.lock().unwrap_or_else(|e| e.into_inner());
                                let n = counts.entry(source_id.to_string()).or_insert(0);
                                *n += 1;
                                log::warn!("scan {source}: empty result #{n}, keeping {existing} existing records");
                                // 连续 3 次(约 15 分钟)空扫描 → 提示路径可能迁移。
                                if *n >= 3 {
                                    details.push(format!("{source}: 连续 {} 次扫描无新数据,路径可能已迁移(检查 {} 或 REASONIX_PROJECTS_DIR 等环境变量)", *n, source_id));
                                    counts.remove(source_id);
                                } else {
                                    details.push(format!("{source}: 扫描结果为空,保留已有 {existing} 条记录"));
                                }
                                continue;
                            }
                            _ => {
                                if let Ok(mut counts) = EMPTY_SCAN_COUNTS.lock() { counts.remove(source_id); }
                            }
                        }
                    } else if let Ok(mut counts) = EMPTY_SCAN_COUNTS.lock() { counts.remove(source_id); }
                    match repository.replace_source(source_id, &output.buckets, &output.sessions) {
                        Ok((_removed, buckets, sessions)) => {
                            successful_sources += 1;
                            details.push(format!("{source}: {} buckets / {} sessions", buckets.inserted + buckets.updated + buckets.unchanged, sessions.inserted + sessions.updated + sessions.unchanged));
                        }
                        Err(error) => {
                            log::error!("scan {source}: replace_source failed: {error}");
                            details.push(format!("{source}: {error}"))
                        },
                    }
                }
                Err(error) => {
                    log::error!("scan {source}: parse failed: {error}");
                    details.push(format!("{source}: {error}"))
                },
            }
        }
    }
    let state = if successful_sources > 0 {
        SyncState { status: "ready".into(), message: Some(details.join(" · ")), last_scan_at: Some(now_ms()) }
    } else {
        SyncState { status: "error".into(), message: Some(if details.is_empty() { "未找到可读取的数据源".into() } else { details.join(" · ") }), last_scan_at: None }
    };
    publish(&app, state);
    crate::tray::refresh_tooltip(&app);
}
