use crate::state::AppCtx;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use tauri::{AppHandle, Emitter, LogicalSize, Manager, PhysicalPosition, Window};

pub const DASHBOARD: &str = "dashboard";
pub const WIDGET: &str = "floating-meter";
const FULL: (f64, f64) = (420.0, 208.0);
const COMPACT: (f64, f64) = (140.0, 56.0);
const TAB: (f64, f64) = (16.0, 51.0);
const SNAP_DISTANCE: f64 = 18.0;
static WIDGET_MOVE_GENERATION: AtomicU64 = AtomicU64::new(0);
/// 折叠/展开滑动进行中:抑制位置持久化与移动修复,滑动只做纯位移。
static WIDGET_SLIDING: AtomicBool = AtomicBool::new(false);

#[derive(Clone, Copy)]
struct ScreenBounds { x: i32, y: i32, width: i32, height: i32 }

pub fn apply_saved_widget_state(app: &AppHandle) {
    let Some(window) = app.get_webview_window(WIDGET) else { return };
    let settings = app.state::<AppCtx>().settings.lock().unwrap().clone();
    let _ = window.set_always_on_top(settings.widget_always_on_top);
    if settings.widget_collapsed {
        // 启动时直接落位成贴边胶囊,不播滑动;widget_x/y 存的是展开
        // 位置,绝不能用来摆放胶囊。
        let target = tab_size(&settings.widget_edge);
        resize_widget(&window, target);
        if let Some(pos) = tab_edge_position(&window, &settings.widget_edge, target) {
            let _ = window.set_position(pos);
        }
    } else {
        resize_widget(&window, widget_size(settings.widget_compact));
        if let (Some(x), Some(y)) = (settings.widget_x, settings.widget_y) {
            let _ = window.set_position(PhysicalPosition::new(x, y));
            clamp_widget_to_monitor(&window);
        }
    }
    if settings.widget_visible { let _ = window.show(); } else { let _ = window.hide(); }
    schedule_widget_frame_repair(app);
}

fn schedule_widget_frame_repair(app: &AppHandle) {
    for delay in [120_u64, 600] {
        let app = app.clone();
        tauri::async_runtime::spawn(async move {
            tokio::time::sleep(std::time::Duration::from_millis(delay)).await;
            ensure_widget_frame(&app);
        });
    }
}

fn clamp_widget_to_monitor(window: &tauri::WebviewWindow) {
    let Ok(position) = window.outer_position() else { return };
    let Ok(size) = window.outer_size() else { return };
    let Ok(monitors) = window.available_monitors() else { return };
    let bounds: Vec<_> = monitors.iter().map(|monitor| ScreenBounds { x: monitor.position().x, y: monitor.position().y, width: monitor.size().width as i32, height: monitor.size().height as i32 }).collect();
    let threshold = (SNAP_DISTANCE * window.scale_factor().unwrap_or(1.0)).round() as i32;
    let next = constrain_position(position, size.width as i32, size.height as i32, &bounds, threshold);
    if next != position { let _ = window.set_position(next); }
}

fn constrain_position(position: PhysicalPosition<i32>, width: i32, height: i32, monitors: &[ScreenBounds], threshold: i32) -> PhysicalPosition<i32> {
    let center_x = position.x + width / 2;
    let center_y = position.y + height / 2;
    let Some(monitor) = monitors.iter().min_by_key(|monitor| {
        let right = monitor.x + monitor.width;
        let bottom = monitor.y + monitor.height;
        let dx = if center_x < monitor.x { monitor.x - center_x } else if center_x > right { center_x - right } else { 0 };
        let dy = if center_y < monitor.y { monitor.y - center_y } else if center_y > bottom { center_y - bottom } else { 0 };
        dx.saturating_mul(dx).saturating_add(dy.saturating_mul(dy))
    }) else { return position };
    let max_x = (monitor.x + monitor.width - width).max(monitor.x);
    let max_y = (monitor.y + monitor.height - height).max(monitor.y);
    let mut x = position.x.clamp(monitor.x, max_x);
    let y = position.y.clamp(monitor.y, max_y);
    if (x - monitor.x).abs() <= threshold { x = monitor.x; }
    if (max_x - x).abs() <= threshold { x = max_x; }
    PhysicalPosition::new(x, y)
}

fn position_after_resize(position: PhysicalPosition<i32>, old_size: (i32, i32), new_size: (i32, i32), monitor: ScreenBounds, threshold: i32) -> PhysicalPosition<i32> {
    let old_max_x = monitor.x + monitor.width - old_size.0;
    let old_max_y = monitor.y + monitor.height - old_size.1;
    let new_max_x = (monitor.x + monitor.width - new_size.0).max(monitor.x);
    let new_max_y = (monitor.y + monitor.height - new_size.1).max(monitor.y);
    let x = if (position.x - monitor.x).abs() <= threshold { monitor.x }
        else if (old_max_x - position.x).abs() <= threshold { new_max_x }
        else { position.x.clamp(monitor.x, new_max_x) };
    let y = if (position.y - monitor.y).abs() <= threshold { monitor.y }
        else if (old_max_y - position.y).abs() <= threshold { new_max_y }
        else { position.y.clamp(monitor.y, new_max_y) };
    PhysicalPosition::new(x, y)
}

fn resize_widget(window: &tauri::WebviewWindow, size: (f64, f64)) {
    let _ = window.set_size(LogicalSize::new(size.0, size.1));
    apply_material(window, size);
}

fn widget_size(compact: bool) -> (f64, f64) { if compact { COMPACT } else { FULL } }

/// 贴边胶囊尺寸:左右边为竖向胶囊,上下边为横向胶囊。
fn tab_size(edge: &str) -> (f64, f64) {
    match edge {
        "top" | "bottom" => (TAB.1, TAB.0),
        _ => TAB,
    }
}

fn resize_widget_preserving_anchor(window: &tauri::WebviewWindow, size: (f64, f64)) {
    let position = window.outer_position().ok();
    let old_size = window.outer_size().ok();
    let monitor = window.current_monitor().ok().flatten();
    resize_widget(window, size);
    let (Some(position), Some(old_size), Some(monitor)) = (position, old_size, monitor) else {
        clamp_widget_to_monitor(window);
        return;
    };
    let scale = window.scale_factor().unwrap_or(1.0);
    let next = position_after_resize(
        position,
        (old_size.width as i32, old_size.height as i32),
        ((size.0 * scale).round() as i32, (size.1 * scale).round() as i32),
        ScreenBounds { x: monitor.position().x, y: monitor.position().y, width: monitor.size().width as i32, height: monitor.size().height as i32 },
        (SNAP_DISTANCE * scale).round() as i32,
    );
    let _ = window.set_position(next);
}

#[cfg(target_os = "windows")]
fn apply_material(window: &tauri::WebviewWindow, size: (f64, f64)) {
    use windows_sys::Win32::Graphics::Dwm::{
        DwmSetWindowAttribute, DWMNCRP_DISABLED, DWMWA_NCRENDERING_POLICY,
    };
    use windows_sys::Win32::Graphics::Gdi::{
        CreateRectRgn, CreateRoundRectRgn, DeleteObject, EqualRgn, GetWindowRgn, SetWindowRgn,
    };
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        GetWindowLongPtrW, SetWindowLongPtrW, SetWindowPos, GWL_STYLE,
        SWP_FRAMECHANGED, SWP_NOACTIVATE, SWP_NOMOVE, SWP_NOSIZE, SWP_NOZORDER,
        WS_CAPTION, WS_MAXIMIZEBOX, WS_MINIMIZEBOX, WS_POPUP, WS_SYSMENU, WS_THICKFRAME,
    };

    let Ok(hwnd) = window.hwnd() else { return };
    let hwnd = hwnd.0 as _;
    unsafe {
        let style = GetWindowLongPtrW(hwnd, GWL_STYLE);
        let framed = (WS_CAPTION | WS_THICKFRAME | WS_MINIMIZEBOX | WS_MAXIMIZEBOX | WS_SYSMENU) as isize;
        let expected_style = (style & !framed) | WS_POPUP as isize;
        if expected_style != style {
            SetWindowLongPtrW(hwnd, GWL_STYLE, expected_style);
        }
        let policy = DWMNCRP_DISABLED;
        let _ = DwmSetWindowAttribute(
            hwnd,
            DWMWA_NCRENDERING_POLICY as u32,
            &policy as *const _ as *const std::ffi::c_void,
            std::mem::size_of_val(&policy) as u32,
        );
        if expected_style != style {
            SetWindowPos(
                hwnd,
                std::ptr::null_mut(),
                0,
                0,
                0,
                0,
                SWP_FRAMECHANGED | SWP_NOMOVE | SWP_NOSIZE | SWP_NOZORDER | SWP_NOACTIVATE,
            );
        }
    }

    // region 必须按「目标尺寸」计算:set_size 是异步的,此刻用 GetWindowRect
    // 读到的是旧矩形——展开时 region 会比新窗口小,把卡片裁到只剩一角。
    // 目标尺寸算出的 region 在 resize 落地前略大于窗口(无害),落地后精确贴合。
    let scale = window.scale_factor().unwrap_or(1.0);
    let width = (size.0 * scale).round() as i32;
    let height = (size.1 * scale).round() as i32;
    if width <= 0 || height <= 0 { return; }
    let logical_radius = 24.0_f64.min(size.0 / 2.0).min(size.1 / 2.0);
    let corner_diameter = (logical_radius * 2.0 * scale).round() as i32;
    let region = unsafe { CreateRoundRectRgn(0, 0, width + 1, height + 1, corner_diameter, corner_diameter) };
    if region.is_null() { return; }
    let current_region = unsafe { CreateRectRgn(0, 0, 0, 0) };
    if !current_region.is_null() {
        let has_region = unsafe { GetWindowRgn(hwnd, current_region) } > 0;
        let region_matches = has_region && unsafe { EqualRgn(region, current_region) } != 0;
        unsafe { DeleteObject(current_region as _) };
        if region_matches {
            unsafe { DeleteObject(region as _) };
            return;
        }
    }
    if unsafe { SetWindowRgn(hwnd, region, 1) } == 0 { unsafe { DeleteObject(region as _) }; }
}
#[cfg(not(target_os = "windows"))]
fn apply_material(_window: &tauri::WebviewWindow, _size: (f64, f64)) {}

pub fn ensure_widget_frame(app: &AppHandle) {
    let Some(window) = app.get_webview_window(WIDGET) else { return };
    let settings = app.state::<AppCtx>().settings.lock().unwrap().clone();
    let size = if settings.widget_collapsed { tab_size(&settings.widget_edge) } else { widget_size(settings.widget_compact) };
    apply_material(&window, size);
    repair_widget_composition(&window);
}

pub fn schedule_widget_move_repair(app: &AppHandle) {
    // 滑动期间每步 set_position 都会触发 Moved,不做中途修复;
    // 滑动结束由调用方统一 ensure_widget_frame。
    if WIDGET_SLIDING.load(Ordering::SeqCst) { return; }
    let generation = WIDGET_MOVE_GENERATION.fetch_add(1, Ordering::Relaxed) + 1;
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        tokio::time::sleep(std::time::Duration::from_millis(90)).await;
        if WIDGET_MOVE_GENERATION.load(Ordering::Relaxed) == generation {
            ensure_widget_frame(&app);
        }
    });
}

pub fn repair_widget_after_focus_change(app: &AppHandle) {
    ensure_widget_frame(app);
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        tokio::time::sleep(std::time::Duration::from_millis(48)).await;
        ensure_widget_frame(&app);
    });
}

#[cfg(target_os = "windows")]
pub fn start_widget_drag(app: &AppHandle) {
    use windows_sys::Win32::Foundation::{POINT, RECT};
    use windows_sys::Win32::UI::Input::KeyboardAndMouse::{GetAsyncKeyState, VK_LBUTTON};
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        GetCursorPos, GetWindowRect, SetWindowPos, SWP_NOACTIVATE, SWP_NOSIZE, SWP_NOZORDER,
    };

    let Some(window) = app.get_webview_window(WIDGET) else { return };
    let Ok(hwnd) = window.hwnd() else { return };
    let hwnd = hwnd.0 as isize;
    let app = app.clone();
    tauri::async_runtime::spawn_blocking(move || unsafe {
        let mut cursor = POINT { x: 0, y: 0 };
        let mut bounds = RECT { left: 0, top: 0, right: 0, bottom: 0 };
        if GetCursorPos(&mut cursor) == 0 || GetWindowRect(hwnd as _, &mut bounds) == 0 { return; }
        let start_cursor = cursor;
        let start_x = bounds.left;
        let start_y = bounds.top;
        while GetAsyncKeyState(VK_LBUTTON as i32) < 0 {
            if GetCursorPos(&mut cursor) != 0 {
                SetWindowPos(
                    hwnd as _,
                    std::ptr::null_mut(),
                    start_x + cursor.x - start_cursor.x,
                    start_y + cursor.y - start_cursor.y,
                    0,
                    0,
                    SWP_NOSIZE | SWP_NOZORDER | SWP_NOACTIVATE,
                );
            }
            std::thread::sleep(std::time::Duration::from_millis(16));
        }
        ensure_widget_frame(&app);
    });
}

#[cfg(not(target_os = "windows"))]
pub fn start_widget_drag(_app: &AppHandle) {}

#[cfg(target_os = "windows")]
fn repair_widget_composition(window: &tauri::WebviewWindow) {
    use windows_sys::Win32::Graphics::Dwm::DwmFlush;
    use windows_sys::Win32::Graphics::Gdi::{
        RedrawWindow, RDW_ALLCHILDREN, RDW_ERASE, RDW_FRAME, RDW_INVALIDATE, RDW_UPDATENOW,
    };

    let _ = window.with_webview(|webview| unsafe {
        let _ = webview.controller().NotifyParentWindowPositionChanged();
    });
    let Ok(hwnd) = window.hwnd() else { return };
    unsafe {
        RedrawWindow(
            hwnd.0 as _,
            std::ptr::null(),
            std::ptr::null_mut(),
            RDW_INVALIDATE | RDW_ERASE | RDW_FRAME | RDW_ALLCHILDREN | RDW_UPDATENOW,
        );
        let _ = DwmFlush();
    }
}

#[cfg(not(target_os = "windows"))]
fn repair_widget_composition(_window: &tauri::WebviewWindow) {}

pub fn show_dashboard(app: &AppHandle) { if let Some(window) = app.get_webview_window(DASHBOARD) { let _ = window.unminimize(); let _ = window.show(); let _ = window.set_focus(); } }
pub fn toggle_widget(app: &AppHandle) { let visible = app.get_webview_window(WIDGET).and_then(|w| w.is_visible().ok()).unwrap_or(false); set_widget_visible(app, !visible); }
pub fn set_widget_visible(app: &AppHandle, visible: bool) {
    if let Some(window) = app.get_webview_window(WIDGET) { if visible { let _ = window.show(); ensure_widget_frame(app); schedule_widget_frame_repair(app); } else { let _ = window.hide(); } }
    let ctx = app.state::<AppCtx>(); ctx.settings.lock().unwrap().widget_visible = visible; let _ = ctx.save_settings(); emit_settings(app);
}

pub fn set_widget_compact(app: &AppHandle, compact: bool) {
    let Some(window) = app.get_webview_window(WIDGET) else { return };
    let collapsed = {
        let ctx = app.state::<AppCtx>();
        let mut settings = ctx.settings.lock().unwrap();
        settings.widget_compact = compact;
        let collapsed = settings.widget_collapsed;
        drop(settings);
        let _ = ctx.save_settings();
        collapsed
    };
    if !collapsed {
        resize_widget_preserving_anchor(&window, widget_size(compact));
    }
    emit_settings(app);
}

/// 计算折叠后胶囊的贴边位置:钉到指定边缘,保留另一轴的当前位置。
fn tab_edge_position(window: &tauri::WebviewWindow, edge: &str, size: (f64, f64)) -> Option<PhysicalPosition<i32>> {
    let monitor = window.current_monitor().ok().flatten()?;
    let scale = window.scale_factor().unwrap_or(1.0);
    let w = (size.0 * scale) as i32;
    let h = (size.1 * scale) as i32;
    let origin = monitor.position();
    let area = monitor.size();
    let max_x = (origin.x + area.width as i32 - w).max(origin.x);
    let max_y = (origin.y + area.height as i32 - h).max(origin.y);
    let position = window.outer_position().unwrap_or(origin.to_owned());
    Some(match edge {
        "left" => PhysicalPosition::new(origin.x, position.y.clamp(origin.y, max_y)),
        "right" => PhysicalPosition::new(max_x, position.y.clamp(origin.y, max_y)),
        "top" => PhysicalPosition::new(position.x.clamp(origin.x, max_x), origin.y),
        _ => PhysicalPosition::new(position.x.clamp(origin.x, max_x), max_y),
    })
}

/// ease-out-cubic:滑动起步快、收尾稳。
fn ease_out_cubic(t: f64) -> f64 {
    let t = t.clamp(0.0, 1.0);
    1.0 - (1.0 - t).powi(3)
}

/// 纯位移滑动:窗口尺寸不变、内容静态,与拖拽同一条 SetWindowPos 路径
/// (拖拽 16ms 步进从不卡顿即是证据)。不触发 WebView reflow,没有任何
/// 前端补偿逻辑——震颤在结构上不可能出现。
async fn slide_widget(window: &tauri::WebviewWindow, from: PhysicalPosition<i32>, to: PhysicalPosition<i32>) {
    const STEPS: i32 = 14;
    const STEP_MS: u64 = 16;
    for step in 1..=STEPS {
        tokio::time::sleep(std::time::Duration::from_millis(STEP_MS)).await;
        let t = ease_out_cubic(step as f64 / STEPS as f64);
        let x = (from.x as f64 + (to.x - from.x) as f64 * t).round() as i32;
        let y = (from.y as f64 + (to.y - from.y) as f64 * t).round() as i32;
        let _ = window.set_position(PhysicalPosition::new(x, y));
    }
    let _ = window.set_position(to);
}

/// 折叠:窗口以全尺寸滑向屏幕边缘,直至只剩胶囊大小的一条留在屏幕内
/// (滑动终点即胶囊最终位置 tab_pos,窗口其余部分在屏幕外——视觉上
/// 卡片"滑进边缘"只留下胶囊,是最自然的收起隐喻)。到位后切胶囊内容
/// (固定定位在窗口左上角,正好是屏幕内的那一小条),最后做唯一一次
/// resize:Windows 缩放锚定左上角,窗口就地缩成胶囊,无需再移动,
/// 不存在任何"先消失再出现"的空窗帧。
pub fn collapse_widget(app: &AppHandle) {
    if WIDGET_SLIDING.load(Ordering::SeqCst) { return; }
    {
        let ctx = app.state::<AppCtx>();
        if ctx.settings.lock().unwrap().widget_collapsed { return; }
    }
    let Some(window) = app.get_webview_window(WIDGET) else { return };
    let Ok(Some(monitor)) = window.current_monitor() else { return };
    let position = window.outer_position().unwrap_or(monitor.position().to_owned());
    let size = window.outer_size().unwrap_or(monitor.size().to_owned());
    let origin = monitor.position(); let area = monitor.size();
    let distances = [("left", (position.x - origin.x).abs()), ("right", (origin.x + area.width as i32 - position.x - size.width as i32).abs()), ("top", (position.y - origin.y).abs()), ("bottom", (origin.y + area.height as i32 - position.y - size.height as i32).abs())];
    let edge = distances.into_iter().min_by_key(|(_, distance)| *distance).map(|v| v.0).unwrap_or("right");
    let target = tab_size(edge);
    let tab_pos = tab_edge_position(&window, edge, target);
    // 保存展开位置:折叠后 persist 会被抑制,必须在此刻落盘,
    // 否则拖完立刻收起再展开会回到旧位置(位置偏离 bug)。
    {
        let ctx = app.state::<AppCtx>();
        let mut settings = ctx.settings.lock().unwrap_or_else(|e| e.into_inner());
        settings.widget_x = Some(position.x);
        settings.widget_y = Some(position.y);
    }
    let app2 = app.clone();
    let edge2 = edge.to_string();
    WIDGET_SLIDING.store(true, Ordering::SeqCst);
    tauri::async_runtime::spawn(async move {
        if let (Some(window), Some(tab_pos)) = (app2.get_webview_window(WIDGET), tab_pos) {
            // 阶段 1:全尺寸滑向边缘,直到只剩胶囊一条在屏幕内
            // (隐藏窗口如启动路径则直接落位,不播滑动)。
            if window.is_visible().unwrap_or(true) {
                slide_widget(&window, position, tab_pos).await;
            } else {
                let _ = window.set_position(tab_pos);
            }
            // 阶段 2:切胶囊内容 + 落盘 + 广播。胶囊固定定位在窗口左上角,
            // 即屏幕内的那一小条,位置与最终落点逐像素重合。
            let ctx = app2.state::<AppCtx>();
            {
                let mut settings = ctx.settings.lock().unwrap_or_else(|e| e.into_inner());
                settings.widget_collapsed = true;
                settings.widget_edge = edge2.clone();
            }
            let _ = ctx.save_settings();
            emit_settings(&app2);
            // 等前端把胶囊渲染出来,再做唯一一次 resize(锚定左上角就地
            // 收缩,胶囊全程可见,无空窗帧)。
            tokio::time::sleep(std::time::Duration::from_millis(90)).await;
            resize_widget(&window, target);
        }
        WIDGET_SLIDING.store(false, Ordering::SeqCst);
        ensure_widget_frame(&app2);
        schedule_widget_frame_repair(&app2);
    });
}

/// 展开:折叠的镜像。先在胶囊位置把窗口撑到展开尺寸(右/下边缘时
/// 窗口大部分在屏幕外,撑大不可见),再切完整卡片(在屏幕外渲染),
/// 最后从边缘滑回展开位置——视觉上卡片从屏幕边缘滑出并 glide 回原位,
/// 胶囊全程静止直至被替换,无跳动、无空窗帧。
pub fn expand_widget(app: &AppHandle) {
    if WIDGET_SLIDING.load(Ordering::SeqCst) { return; }
    let (edge, compact, saved) = {
        let ctx = app.state::<AppCtx>();
        let settings = ctx.settings.lock().unwrap();
        if !settings.widget_collapsed { return; }
        (settings.widget_edge.clone(), settings.widget_compact, (settings.widget_x, settings.widget_y))
    };
    let expanded_size = widget_size(compact);
    let Some(window) = app.get_webview_window(WIDGET) else { return };
    let Ok(Some(monitor)) = window.current_monitor() else { return };
    let origin = monitor.position(); let area = monitor.size();
    // 展开位置:优先恢复折叠前刚保存的展开位置;若从未展开过,则钉到边缘居中。
    let scale = window.scale_factor().unwrap_or(1.0);
    let w = (expanded_size.0 * scale) as i32;
    let h = (expanded_size.1 * scale) as i32;
    let max_x = (origin.x + area.width as i32 - w).max(origin.x);
    let max_y = (origin.y + area.height as i32 - h).max(origin.y);
    let to_pos = match saved {
        (Some(x), Some(y)) => PhysicalPosition::new(x.clamp(origin.x, max_x), y.clamp(origin.y, max_y)),
        _ => match edge.as_str() {
            "left" => PhysicalPosition::new(origin.x, origin.y + (area.height as i32 - h) / 2),
            "right" => PhysicalPosition::new(max_x, origin.y + (area.height as i32 - h) / 2),
            "top" => PhysicalPosition::new(origin.x + (area.width as i32 - w) / 2, origin.y),
            _ => PhysicalPosition::new(origin.x + (area.width as i32 - w) / 2, max_y),
        },
    };
    let app2 = app.clone();
    WIDGET_SLIDING.store(true, Ordering::SeqCst);
    tauri::async_runtime::spawn(async move {
        if let Some(window) = app2.get_webview_window(WIDGET) {
            // 阶段 1:原地撑大窗口(透明部分不可见)。
            resize_widget(&window, expanded_size);
            // 阶段 2:切完整卡片 + 落盘 + 广播,等一拍到渲染完成。
            let ctx = app2.state::<AppCtx>();
            ctx.settings.lock().unwrap_or_else(|e| e.into_inner()).widget_collapsed = false;
            let _ = ctx.save_settings();
            emit_settings(&app2);
            tokio::time::sleep(std::time::Duration::from_millis(90)).await;
            // 阶段 3:从边缘滑回展开位置。
            let from = window.outer_position().unwrap_or(to_pos);
            slide_widget(&window, from, to_pos).await;
        }
        WIDGET_SLIDING.store(false, Ordering::SeqCst);
        ensure_widget_frame(&app2);
        schedule_widget_frame_repair(&app2);
    });
}

static LAST_PERSIST_MS: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

pub fn persist_widget_position(window: &Window, position: PhysicalPosition<i32>) {
    if WIDGET_SLIDING.load(Ordering::SeqCst) { return; }
    let ctx = window.app_handle().state::<AppCtx>();
    if let (Ok(size), Ok(monitors)) = (window.outer_size(), window.available_monitors()) {
        let bounds: Vec<_> = monitors.iter().map(|monitor| ScreenBounds { x: monitor.position().x, y: monitor.position().y, width: monitor.size().width as i32, height: monitor.size().height as i32 }).collect();
        let threshold = (SNAP_DISTANCE * window.scale_factor().unwrap_or(1.0)).round() as i32;
        let next = constrain_position(position, size.width as i32, size.height as i32, &bounds, threshold);
        if next != position { let _ = window.set_position(next); return; }
    }
    let now_ms = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|value| value.as_millis() as u64).unwrap_or(0);
    if now_ms.saturating_sub(LAST_PERSIST_MS.load(std::sync::atomic::Ordering::Relaxed)) < 500 { return; }
    LAST_PERSIST_MS.store(now_ms, std::sync::atomic::Ordering::Relaxed);
    let mut settings = ctx.settings.lock().unwrap();
    if settings.widget_collapsed { return; }
    settings.widget_x = Some(position.x); settings.widget_y = Some(position.y); drop(settings); let _ = ctx.save_settings();
}
pub fn emit_settings(app: &AppHandle) {
    let mut settings = app.state::<AppCtx>().settings.lock().unwrap_or_else(|e| e.into_inner()).clone();
    // 与 get_settings 保持一致:广播时不带 SMTP 授权码明文,前端留空 = 不修改。
    settings.report_smtp_password.clear();
    let _ = app.emit("settings-updated", settings);
}

#[cfg(test)]
mod tests {
    use super::*;
    use tauri::PhysicalPosition;

    const SCREEN: [ScreenBounds; 1] = [ScreenBounds { x: 0, y: 0, width: 1920, height: 1080 }];

    #[test]
    fn snaps_to_left_and_right_edges() {
        assert_eq!(constrain_position(PhysicalPosition::new(12, 200), 540, 190, &SCREEN, 18).x, 0);
        assert_eq!(constrain_position(PhysicalPosition::new(1370, 200), 540, 190, &SCREEN, 18).x, 1380);
    }

    #[test]
    fn never_leaves_the_monitor() {
        assert_eq!(constrain_position(PhysicalPosition::new(-300, -40), 540, 190, &SCREEN, 18), PhysicalPosition::new(0, 0));
        assert_eq!(constrain_position(PhysicalPosition::new(1900, 1040), 540, 190, &SCREEN, 18), PhysicalPosition::new(1380, 890));
    }

    #[test]
    fn selects_fixed_size_for_each_widget_mode() {
        assert_eq!(widget_size(false), FULL);
        assert_eq!(widget_size(true), COMPACT);
    }

    #[test]
    fn tab_is_horizontal_on_top_bottom_edges_vertical_on_sides() {
        assert_eq!(tab_size("left"), TAB);
        assert_eq!(tab_size("right"), TAB);
        assert_eq!(tab_size("top"), (TAB.1, TAB.0));
        assert_eq!(tab_size("bottom"), (TAB.1, TAB.0));
    }

    #[test]
    fn resizing_keeps_the_widget_on_its_snapped_edge() {
        assert_eq!(
            position_after_resize(PhysicalPosition::new(1380, 200), (540, 189), (210, 84), SCREEN[0], 18),
            PhysicalPosition::new(1710, 200),
        );
        assert_eq!(
            position_after_resize(PhysicalPosition::new(0, 891), (540, 189), (210, 84), SCREEN[0], 18),
            PhysicalPosition::new(0, 996),
        );
    }

    #[test]
    fn ease_out_cubic_is_monotonic_and_exact_at_endpoints() {
        assert_eq!(ease_out_cubic(0.0), 0.0);
        assert_eq!(ease_out_cubic(1.0), 1.0);
        assert_eq!(ease_out_cubic(-0.5), 0.0);
        assert_eq!(ease_out_cubic(1.5), 1.0);
        let mut previous = 0.0;
        for step in 1..=14 {
            let value = ease_out_cubic(step as f64 / 14.0);
            assert!(value > previous, "曲线必须单调递增");
            previous = value;
        }
        assert!((ease_out_cubic(0.5) - 0.875).abs() < 1e-9);
    }
}
