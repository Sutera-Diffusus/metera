use crate::state::AppCtx;
use chrono::{Duration, Local, SecondsFormat, TimeZone, Utc};
use metera_core::usage::UsageBucket;
use tauri::{menu::{Menu, MenuItem}, tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent}, AppHandle, Manager};

pub fn create(app:&AppHandle)->tauri::Result<()>{
    let dashboard=MenuItem::with_id(app,"dashboard","打开仪表盘",true,None::<&str>)?;
    let widget=MenuItem::with_id(app,"widget","显示/隐藏浮窗",true,None::<&str>)?;
    let scan=MenuItem::with_id(app,"scan","更新数据",true,None::<&str>)?;
    let quit=MenuItem::with_id(app,"quit","退出 Metera",true,None::<&str>)?;
    let menu=Menu::with_items(app,&[&dashboard,&widget,&scan,&quit])?;
    let icon=tauri::image::Image::from_bytes(include_bytes!("../icons/tray-icon.png"))?;
    TrayIconBuilder::with_id("metera").tooltip(tooltip_text(app)).icon(icon).menu(&menu).show_menu_on_left_click(false)
      .on_menu_event(|app,event|match event.id.as_ref(){"dashboard"=>crate::windows::show_dashboard(app),"widget"=>crate::windows::toggle_widget(app),"scan"=>{let handle=app.clone();tauri::async_runtime::spawn(async move{crate::services::local_scanner::run(handle).await;});},"quit"=>app.exit(0),_=>{}})
      .on_tray_icon_event(|tray,event|{if let TrayIconEvent::Click{button:MouseButton::Left,button_state:MouseButtonState::Up,..}=event{crate::windows::show_dashboard(tray.app_handle());}}).build(app)?;Ok(())
}

pub fn refresh_tooltip(app: &AppHandle) {
    let handle = app.clone();
    tauri::async_runtime::spawn_blocking(move || {
        let text = tooltip_text(&handle);
        if let Some(tray) = handle.tray_by_id("metera") {
            let _ = tray.set_tooltip(Some(text));
        }
    });
}

fn tooltip_text(app: &AppHandle) -> String {
    let ctx = app.state::<AppCtx>();
    let settings = ctx.settings.lock().unwrap().clone();
    if !settings.show_tokens_in_tray && !settings.show_cost_in_tray {
        return "Metera".into();
    }

    let now = Local::now();
    let Some(start) = Local.from_local_datetime(&now.date_naive().and_hms_opt(0, 0, 0).unwrap()).earliest() else {
        return "Metera".into();
    };
    let end = start + Duration::days(1);
    let buckets = ctx.usage.lock().unwrap().buckets_between(
        &start.with_timezone(&Utc).to_rfc3339_opts(SecondsFormat::Millis, true),
        &end.with_timezone(&Utc).to_rfc3339_opts(SecondsFormat::Millis, true),
    ).unwrap_or_default();
    let tokens = buckets.iter().map(bucket_tokens).sum::<i64>();
    let cost = buckets.iter().filter_map(estimate_cost).sum::<f64>();

    compose_tooltip(settings.show_tokens_in_tray, settings.show_cost_in_tray, tokens, cost)
}

fn compose_tooltip(show_tokens: bool, show_cost: bool, tokens: i64, cost: f64) -> String {
    let mut lines = vec!["Metera".to_string()];
    if show_tokens { lines.push(format!("今日 Token: {}", format_tokens(tokens))); }
    if show_cost { lines.push(format!("今日预估费用: ${cost:.2}")); }
    lines.join("\n")
}

fn bucket_tokens(bucket: &UsageBucket) -> i64 {
    bucket.input_tokens + bucket.cached_input_tokens + bucket.output_tokens + bucket.reasoning_output_tokens
}

fn estimate_cost(bucket: &UsageBucket) -> Option<f64> {
    let value = format!("{} {}", bucket.source, bucket.model).to_ascii_lowercase();
    let (input, cached, output) = if value.contains("deepseek-v4-pro") || value.contains("deepseek-reasoner") {
        (0.435, 0.003625, 0.87)
    } else if value.contains("deepseek-v4-flash") || value.contains("deepseek") {
        (0.14, 0.0028, 0.28)
    } else if value.contains("gpt-5.6-sol") || value.contains("gpt-5.5") {
        (5.0, 0.5, 30.0)
    } else if value.contains("gpt-5.6-terra") {
        (2.0, 0.2, 12.0)
    } else if value.contains("gpt-5.6-luna") {
        (0.2, 0.02, 1.2)
    } else if value.contains("gpt-5.3-codex") {
        (1.75, 0.175, 14.0)
    } else if value.contains("gpt-5-codex") {
        (1.25, 0.125, 10.0)
    } else if value.contains("kimi") || value.contains("k3") {
        (3.0, 0.3, 15.0)
    } else if value.contains("glm-5.2") {
        (1.4, 0.26, 4.4)
    } else {
        return None;
    };
    Some(bucket.input_tokens as f64 / 1e6 * input
        + bucket.cached_input_tokens as f64 / 1e6 * cached
        + (bucket.output_tokens + bucket.reasoning_output_tokens) as f64 / 1e6 * output)
}

fn format_tokens(value: i64) -> String {
    let value = value.max(0) as f64;
    if value >= 1e9 { format!("{:.1}B", value / 1e9) }
    else if value >= 1e6 { format!("{:.1}M", value / 1e6) }
    else if value >= 1e3 { format!("{:.1}K", value / 1e3) }
    else { format!("{value:.0}") }
}

#[cfg(test)]
mod tests {
    use super::{compose_tooltip, format_tokens};

    #[test]
    fn formats_tray_token_totals() {
        assert_eq!(format_tokens(132_600_000), "132.6M");
        assert_eq!(format_tokens(642_700), "642.7K");
    }

    #[test]
    fn composes_tray_tooltip_from_settings() {
        assert_eq!(compose_tooltip(false, false, 132_600_000, 111.05), "Metera");
        assert_eq!(compose_tooltip(true, false, 132_600_000, 111.05), "Metera\n今日 Token: 132.6M");
        assert_eq!(compose_tooltip(false, true, 132_600_000, 111.05), "Metera\n今日预估费用: $111.05");
        assert_eq!(compose_tooltip(true, true, 132_600_000, 111.05), "Metera\n今日 Token: 132.6M\n今日预估费用: $111.05");
    }
}
