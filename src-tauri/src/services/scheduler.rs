use crate::state::AppCtx;
use std::time::Duration;
use tauri::{AppHandle, Manager};

pub fn start(app: AppHandle) {
    if let Some(task) = app.state::<AppCtx>().scheduler_task.lock().unwrap().take() { task.abort(); }
    let worker_app = app.clone();
    let task = tauri::async_runtime::spawn(async move {
        loop {
            crate::services::local_scanner::run(worker_app.clone()).await;
            tokio::time::sleep(Duration::from_secs(300)).await;
        }
    });
    *app.state::<AppCtx>().scheduler_task.lock().unwrap() = Some(task);
}
