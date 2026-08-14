use metera_core::usage::UsageRepository;
use serde::{Deserialize, Serialize};
use std::{collections::HashMap, path::PathBuf, sync::Mutex};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct AppSettings {
    pub widget_visible: bool,
    pub widget_always_on_top: bool,
    pub widget_compact: bool,
    pub widget_x: Option<i32>,
    pub widget_y: Option<i32>,
    pub widget_metric: String,
    pub widget_collapsed: bool,
    pub widget_edge: String,
    pub pinned_quota_providers: Vec<String>,
    pub provider_aliases: HashMap<String, String>,
    /// 订阅档位手动覆盖：provider → pricing_data plans 中的 plan 名（如 "Plus"、"Allegretto"）。
    /// 自动检测不可靠（codex plan_type 常为 null、Kimi 需登录态）时由用户手动指定。
    pub plan_overrides: HashMap<String, String>,
    pub accent: String,
    pub density: String,
    pub include_project_names: bool,
    pub show_cost_in_tray: bool,
    pub show_tokens_in_tray: bool,
    pub daily_report_enabled: bool,
    pub report_email: String,
    pub report_smtp_host: String,
    pub report_smtp_port: u16,
    pub report_smtp_password: String,
    pub report_send_time: String,
    pub report_last_sent_at: Option<String>,
    pub report_last_error: Option<String>,
}

impl Default for AppSettings {
    fn default() -> Self {
        Self {
            widget_visible: true,
            widget_always_on_top: true,
            widget_compact: false,
            widget_x: None,
            widget_y: None,
            widget_metric: "tokens".into(),
            widget_collapsed: false,
            widget_edge: "right".into(),
            pinned_quota_providers: vec!["codex".into(), "kimi".into()],
            provider_aliases: HashMap::new(),
            plan_overrides: HashMap::new(),
            accent: "mint".into(),
            density: "comfortable".into(),
            include_project_names: true,
            show_cost_in_tray: true,
            show_tokens_in_tray: false,
            daily_report_enabled: false,
            report_email: String::new(),
            report_smtp_host: String::new(),
            report_smtp_port: 465,
            report_smtp_password: String::new(),
            report_send_time: "08:00".into(),
            report_last_sent_at: None,
            report_last_error: None,
        }
    }
}

#[derive(Debug, Clone, Serialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct SyncState {
    pub status: String,
    pub message: Option<String>,
    pub last_scan_at: Option<u64>,
}

pub struct AppCtx {
    pub data_dir: PathBuf,
    pub settings_path: PathBuf,
    pub codex_home: PathBuf,
    pub codex_cache_dir: PathBuf,
    pub home_dir: PathBuf,
    pub kimi_code_dir: PathBuf,
    pub workbuddy_projects_dir: PathBuf,
    pub reasonix_projects_dir: PathBuf,
    pub dsh_home: PathBuf,
    pub dsh_sessions_dir: PathBuf,
    pub usage: Mutex<UsageRepository>,
    pub settings: Mutex<AppSettings>,
    pub sync_state: Mutex<SyncState>,
    pub sync_running: tokio::sync::Mutex<()>,
    pub scheduler_task: Mutex<Option<tauri::async_runtime::JoinHandle<()>>>,
    pub report_task: Mutex<Option<tauri::async_runtime::JoinHandle<()>>>,
}

/// 数据源路径解析结果（可重入：每次扫描时重新探测，感知路径迁移）。
#[derive(Debug, Clone)]
pub struct SourcePaths {
    pub codex_home: PathBuf,
    pub kimi_code_dir: PathBuf,
    pub kimi_legacy_dir: PathBuf,
    pub workbuddy_projects_dir: PathBuf,
    pub reasonix_projects_dir: PathBuf,
    pub dsh_home: PathBuf,
    pub dsh_sessions_dir: PathBuf,
}

/// 解析各数据源路径。优先环境变量，其次探测常见迁移位置，最后回退通用默认位置。
/// 独立为纯函数以便每次扫描重新调用，路径迁移后无需重启即可自动识别。
pub fn resolve_paths(home: &PathBuf) -> SourcePaths {
    let codex_home = std::env::var_os("CODEX_HOME")
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
        .unwrap_or_else(|| home.join(".codex"));
    let kimi_code_dir = std::env::var_os("KIMI_CODE_HOME")
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
        .or_else(|| {
            let migrated = PathBuf::from(r"D:\Zcode\.kimi-code");
            migrated.join("config.toml").exists().then_some(migrated)
        })
        .unwrap_or_else(|| home.join(".kimi-code"));
    let reasonix_projects_dir = std::env::var_os("REASONIX_PROJECTS_DIR")
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
        .or_else(|| {
            // 迁移后的新位置：projects 下任意含 sessions/*.telemetry.json 的项目目录都算有效,
            // 不绑定某个固定工作区名(未来工作区改名/新增都不会失效)。
            let migrated = PathBuf::from(r"D:\reasonix\projects");
            let has_reasonix_data = std::fs::read_dir(&migrated).ok()
                .into_iter()
                .flatten()
                .filter_map(Result::ok)
                .filter(|entry| entry.file_type().map(|t| t.is_dir()).unwrap_or(false))
                .any(|entry| {
                    entry.path().join("sessions").is_dir()
                        && std::fs::read_dir(entry.path().join("sessions")).ok()
                            .into_iter()
                            .flatten()
                            .filter_map(Result::ok)
                            .any(|file| file.file_name().to_string_lossy().ends_with(".telemetry.json"))
                });
            has_reasonix_data.then_some(migrated)
        })
        .unwrap_or_else(|| home.join("AppData").join("Roaming").join("Reasonix").join("projects"));
    let dsh_home = std::env::var_os("DSH_HOME")
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
        .unwrap_or_else(|| home.join(".dsh"));
    let dsh_sessions_dir = std::env::var_os("DSH_SESSIONS_DIR")
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
        .unwrap_or_else(|| dsh_home.join("sessions"));
    SourcePaths {
        codex_home,
        kimi_code_dir,
        kimi_legacy_dir: home.join(".kimi"),
        workbuddy_projects_dir: home.join(".workbuddy").join("projects"),
        reasonix_projects_dir,
        dsh_home,
        dsh_sessions_dir,
    }
}

impl AppCtx {
    pub fn new() -> Result<Self, String> {
        let data_dir = std::env::var_os("METERA_DATA_DIR")
            .filter(|value| !value.is_empty())
            .map(PathBuf::from)
            .unwrap_or_else(|| {
                // 兼容现有安装:优先 D:\MeteraData(本机既有数据),否则回退用户数据目录,消除无 D 盘机器启动崩溃。
                let legacy = PathBuf::from(r"D:\MeteraData");
                if legacy.is_dir() {
                    legacy
                } else {
                    dirs::data_dir().unwrap_or_else(|| PathBuf::from(".")).join("Metera")
                }
            });
        std::fs::create_dir_all(&data_dir).map_err(|error| error.to_string())?;
        let settings_path = data_dir.join("settings.json");
        let settings = std::fs::read_to_string(&settings_path)
            .ok()
            .and_then(|raw| match serde_json::from_str::<AppSettings>(&raw) {
                Ok(parsed) => Some(parsed),
                Err(_) => {
                    // 配置文件损坏:先备份原文件,避免后续保存覆盖掉可恢复的现场。
                    let backup = settings_path.with_extension("json.corrupt");
                    let _ = std::fs::copy(&settings_path, &backup);
                    None
                }
            })
            .unwrap_or_default();
        let home = dirs::home_dir().unwrap_or_else(|| PathBuf::from("."));
        let paths = resolve_paths(&home);
        let usage = UsageRepository::open(data_dir.join("metera.db"))
            .map_err(|error| error.to_string())?;
        Ok(Self {
            data_dir: data_dir.clone(),
            settings_path,
            codex_cache_dir: data_dir.join("cache").join("codex"),
            codex_home: paths.codex_home.clone(),
            home_dir: home.clone(),
            kimi_code_dir: paths.kimi_code_dir.clone(),
            workbuddy_projects_dir: paths.workbuddy_projects_dir.clone(),
            reasonix_projects_dir: paths.reasonix_projects_dir.clone(),
            dsh_home: paths.dsh_home.clone(),
            dsh_sessions_dir: paths.dsh_sessions_dir.clone(),
            usage: Mutex::new(usage),
            settings: Mutex::new(settings),
            sync_state: Mutex::new(SyncState { status: "idle".into(), ..Default::default() }),
            sync_running: tokio::sync::Mutex::new(()),
            scheduler_task: Mutex::new(None),
            report_task: Mutex::new(None),
        })
    }

    pub fn save_settings(&self) -> Result<(), String> {
        let bytes = serde_json::to_vec_pretty(&*self.settings.lock().unwrap())
            .map_err(|error| error.to_string())?;
        // 唯一临时文件名(进程号+时间戳),避免多个并发写入互相覆盖临时文件。
        let stamp = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH)
            .map(|value| value.as_millis()).unwrap_or(0);
        let temporary = self.settings_path.with_extension(format!("{}.{}.json.tmp", std::process::id(), stamp));
        std::fs::write(&temporary, bytes).map_err(|error| error.to_string())?;
        std::fs::rename(&temporary, &self.settings_path).map_err(|error| {
            let _ = std::fs::remove_file(&temporary);
            error.to_string()
        })
    }

    pub fn hostname() -> String {
        let value = gethostname::gethostname().to_string_lossy().trim().to_string();
        if value.is_empty() { "desktop".into() } else { value }
    }
}
