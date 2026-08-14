mod commands;
mod services;
mod state;
mod tray;
mod windows;

use state::AppCtx;
use tauri::{Manager, WindowEvent};

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info")).init();
    tauri::Builder::default()
        .manage(AppCtx::new().expect("failed to initialize Metera data store"))
        .plugin(tauri_plugin_single_instance::init(|app, _, _| windows::show_dashboard(app)))
        .setup(|app| {
            tray::create(app.handle())?;
            windows::apply_saved_widget_state(app.handle());
            services::scheduler::start(app.handle().clone());
            services::report::start_daily_report(app.handle().clone());
            Ok(())
        })
        .on_window_event(|window, event| match event {
            WindowEvent::CloseRequested { api, .. } if window.label() == windows::DASHBOARD => {
                api.prevent_close();
                let _ = window.hide();
            }
            WindowEvent::CloseRequested { api, .. } if window.label() == windows::WIDGET => {
                api.prevent_close();
                windows::set_widget_visible(window.app_handle(), false);
            }
            WindowEvent::Moved(position) if window.label() == windows::WIDGET => {
                windows::persist_widget_position(window, *position);
                windows::schedule_widget_move_repair(window.app_handle());
            }
            WindowEvent::Focused(false) if window.label() == windows::WIDGET => {
                windows::repair_widget_after_focus_change(window.app_handle());
            }
            _ => {}
        })
        .invoke_handler(tauri::generate_handler![
            commands::get_app_status,
            commands::fetch_usage,
            commands::get_exchange_rate,
            commands::trigger_scan,
            commands::get_scan_state,
            commands::get_agent_activity,
            commands::get_quota_status,
            commands::bind_account,
            commands::get_settings,
            commands::set_settings,
            commands::set_email_settings,
            commands::send_test_email,
            commands::send_report_now,
            commands::get_launch_at_login,
            commands::set_launch_at_login,
            commands::show_dashboard,
            commands::toggle_widget,
            commands::close_widget,
            commands::collapse_widget,
            commands::expand_widget,
            commands::set_widget_compact,
            commands::start_widget_drag,
            commands::quit_app,
        ])
        .build(tauri::generate_context!())
        .expect("failed to build Metera")
        .run(|_, event| {
            if let tauri::RunEvent::ExitRequested { api, code, .. } = event {
                if code.is_none() { api.prevent_exit(); }
            }
        });
}
