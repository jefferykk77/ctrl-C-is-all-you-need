mod win32;

use std::sync::Mutex;
use std::thread;
use std::time::Duration;
use tauri::{AppHandle, Emitter, Manager};
use windows_sys::Win32::Foundation::{HWND, LPARAM, WPARAM, LRESULT};
use windows_sys::Win32::UI::WindowsAndMessaging::{
    SetWindowLongPtrW, PostMessageW, GetForegroundWindow, GetAncestor,
    GWLP_WNDPROC, GA_ROOT, WNDPROC
};
use windows_sys::Win32::UI::Input::KeyboardAndMouse::{
    RegisterHotKey, UnregisterHotKey, MOD_CONTROL, MOD_SHIFT
};
use windows_sys::Win32::System::DataExchange::{
    AddClipboardFormatListener, RemoveClipboardFormatListener
};


const WM_HOTKEY: u32 = 0x0312;
const WM_CLIPBOARDUPDATE: u32 = 0x031D;
const WM_DESTROY: u32 = 0x0002;
const WM_CUSTOM_UNPAUSE: u32 = 0x0400 + 100; // WM_USER + 100

struct SharedState {
    hwnd_a: Option<isize>,
    hwnd_b: Option<isize>,
    title_a: String,
    title_b: String,
    is_active: bool,
    is_paused: bool,
    main_window_hwnd: Option<isize>,
    is_dragging_detect: bool,
}

static STATE: Mutex<SharedState> = Mutex::new(SharedState {
    hwnd_a: None,
    hwnd_b: None,
    title_a: String::new(),
    title_b: String::new(),
    is_active: false,
    is_paused: false,
    main_window_hwnd: None,
    is_dragging_detect: false,
});

static APP_HANDLE: Mutex<Option<AppHandle>> = Mutex::new(None);
static mut ORIGINAL_WNDPROC: WNDPROC = None;

fn debug_log(msg: &str) {
    if let Ok(mut file) = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open("C:\\Users\\Jeffery\\.gemini\\antigravity\\scratch\\debug.log")
    {
        use std::io::Write;
        let _ = writeln!(file, "{}", msg);
    }
}

// Custom WndProc Subclass Callback to process clipboard and global hotkeys natively
unsafe extern "system" fn subclass_wndproc(
    hwnd: HWND,
    msg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    match msg {
        WM_CLIPBOARDUPDATE => {
            let mut state = STATE.lock().unwrap();
            debug_log(&format!("WM_CLIPBOARDUPDATE received: is_active={}, is_paused={}", state.is_active, state.is_paused));
            if state.is_active && !state.is_paused {
                let fore_hwnd = GetForegroundWindow();
                let root_hwnd = GetAncestor(fore_hwnd, GA_ROOT);
                debug_log(&format!("  fore_hwnd={}, root_hwnd={}", fore_hwnd, root_hwnd));
                
                let is_from_a = Some(root_hwnd) == state.hwnd_a;
                let is_from_b = Some(root_hwnd) == state.hwnd_b;
                debug_log(&format!("  is_from_a={}, is_from_b={}, hwnd_a={:?}, hwnd_b={:?}", is_from_a, is_from_b, state.hwnd_a, state.hwnd_b));
                
                if is_from_a || is_from_b {
                    let source = root_hwnd;
                    let target = if is_from_a { state.hwnd_b.unwrap() } else { state.hwnd_a.unwrap() };
                    
                    state.is_paused = true;
                    let main_hwnd = state.main_window_hwnd.unwrap_or(0);
                    let app_handle = APP_HANDLE.lock().unwrap().clone();
                    if let Some(app) = app_handle {
                        let _ = app.run_on_main_thread(move || {
                            execute_paste(source, target, false, main_hwnd);
                        });
                    }
                }
            }
            0
        }
        WM_HOTKEY => {
            let hotkey_id = wparam as i32;
            debug_log(&format!("WM_HOTKEY received: id={}", hotkey_id));
            if hotkey_id == 101 {
                let mut state = STATE.lock().unwrap();
                if state.is_active && !state.is_paused {
                    let fore_hwnd = GetForegroundWindow();
                    let root_hwnd = GetAncestor(fore_hwnd, GA_ROOT);
                    
                    let is_from_a = Some(root_hwnd) == state.hwnd_a;
                    let is_from_b = Some(root_hwnd) == state.hwnd_b;
                    
                    if is_from_a || is_from_b {
                        let source = root_hwnd;
                        let target = if is_from_a { state.hwnd_b.unwrap() } else { state.hwnd_a.unwrap() };
                        
                        state.is_paused = true;
                        let main_hwnd = state.main_window_hwnd.unwrap_or(0);
                        let app_handle = APP_HANDLE.lock().unwrap().clone();
                        if let Some(app) = app_handle {
                            let _ = app.run_on_main_thread(move || {
                                execute_paste(source, target, true, main_hwnd);
                            });
                        }
                    }
                }
            }
            0
        }
        WM_CUSTOM_UNPAUSE => {
            let mut state = STATE.lock().unwrap();
            debug_log("WM_CUSTOM_UNPAUSE received - unpausing clipboard");
            state.is_paused = false;
            0
        }
        0x0232 => { // WM_EXITSIZEMOVE
            let mut state = STATE.lock().unwrap();
            if state.is_dragging_detect {
                state.is_dragging_detect = false;
                let main_hwnd = state.main_window_hwnd.unwrap_or(0);
                let app_handle = APP_HANDLE.lock().unwrap().clone();
                
                // Get global cursor position
                let mut pt = windows_sys::Win32::Foundation::POINT { x: 0, y: 0 };
                unsafe {
                    windows_sys::Win32::UI::WindowsAndMessaging::GetCursorPos(&mut pt);
                }
                
                thread::spawn(move || {
                    unsafe {
                        // 1. Hide our window
                        windows_sys::Win32::UI::WindowsAndMessaging::ShowWindow(main_hwnd, 0); // SW_HIDE = 0
                        thread::sleep(Duration::from_millis(25));
                        
                        // 2. Probe target window
                        let target = win32::get_window_under_cursor(pt.x, pt.y);
                        
                        // 3. Show window again
                        windows_sys::Win32::UI::WindowsAndMessaging::ShowWindow(main_hwnd, 5); // SW_SHOW = 5
                        win32::set_window_opacity(main_hwnd, 1.0);
                        
                        // 4. Bind target
                        if let Some((hwnd, title)) = target {
                            if hwnd != main_hwnd {
                                let mut state = STATE.lock().unwrap();
                                if Some(hwnd) != state.hwnd_a && Some(hwnd) != state.hwnd_b {
                                    if state.hwnd_a.is_none() {
                                        state.hwnd_a = Some(hwnd);
                                        state.title_a = title;
                                    } else if state.hwnd_b.is_none() {
                                        state.hwnd_b = Some(hwnd);
                                        state.title_b = title;
                                    } else {
                                        state.hwnd_a = Some(hwnd);
                                        state.title_a = title;
                                    }
                                    
                                    // Emit event to frontend
                                    if let Some(app) = app_handle {
                                        let _ = app.emit("slots-updated", ());
                                    }
                                }
                            }
                        }
                    }
                });
            }
            if let Some(orig) = ORIGINAL_WNDPROC {
                orig(hwnd, msg, wparam, lparam)
            } else {
                0
            }
        }
        WM_DESTROY => {
            RemoveClipboardFormatListener(hwnd);
            UnregisterHotKey(hwnd, 101);
            if let Some(orig) = ORIGINAL_WNDPROC {
                orig(hwnd, msg, wparam, lparam)
            } else {
                0
            }
        }
        _ => {
            if let Some(orig) = ORIGINAL_WNDPROC {
                orig(hwnd, msg, wparam, lparam)
            } else {
                0
            }
        }
    }
}

// Background thread executor for focus switching and paste emulation
fn execute_paste(source: HWND, target: HWND, prepend_newline: bool, main_hwnd: HWND) {
    debug_log(&format!("execute_paste started: prepend_newline={}", prepend_newline));
    let mut original_text = String::new();
    let mut modified = false;
    
    if prepend_newline {
        win32::send_copy();
        thread::sleep(Duration::from_millis(40)); // Wait for clipboard update (40ms to match Python)
    }
    
    if let Some(text) = win32::read_clipboard_text() {
        original_text = text;
        debug_log(&format!("  read clipboard text: len={}", original_text.len()));
    } else {
        debug_log("  failed to read clipboard text");
    }
    
    if prepend_newline && !original_text.is_empty() {
        let modified_text = format!("\n{}", original_text);
        if win32::write_clipboard_text(&modified_text) {
            modified = true;
        }
    }
    
    // Switch focus to target window
    win32::force_foreground(target);
    thread::sleep(Duration::from_millis(45)); // 45ms focus transition
    unsafe {
        debug_log(&format!("  foreground window after focus switch: actual={}, expected={}", GetForegroundWindow(), target));
    }
    
    // Send Paste
    debug_log("  sending paste keys");
    win32::send_paste();
    thread::sleep(Duration::from_millis(35)); // 35ms paste complete
    
    // Restore clipboard if we modified it
    if modified {
        win32::write_clipboard_text(&original_text);
    }
    
    // Switch focus back to source window
    win32::force_foreground(source);
    thread::sleep(Duration::from_millis(45)); // 45ms focus transition
    unsafe {
        debug_log(&format!("  foreground window after focus back: actual={}, expected={}", GetForegroundWindow(), source));
    }
    
    debug_log("execute_paste finished, posting unpause");
    // Post message back to main window thread to unpause.
    // Windows guarantees that queued WM_CLIPBOARDUPDATE messages are processed
    // before this custom message since they were posted earlier.
    unsafe {
        if main_hwnd != 0 {
            PostMessageW(main_hwnd, WM_CUSTOM_UNPAUSE, 0, 0);
        }
    }
}

// Tauri commands to bridge with web frontend
#[tauri::command]
fn start_window_drag(window: tauri::Window) {
    let _ = window.start_dragging();
}

#[tauri::command]
fn start_drag_detect(window: tauri::Window) {
    let mut state = STATE.lock().unwrap();
    state.is_dragging_detect = true;
    if let Ok(hwnd) = window.hwnd() {
        win32::set_window_opacity(hwnd.0 as isize, 0.65);
    }
}

#[tauri::command]
fn set_slot(slot: String, hwnd: Option<isize>, title: Option<String>) {
    let mut state = STATE.lock().unwrap();
    let title_str = title.unwrap_or_default();
    if slot == "A" {
        state.hwnd_a = hwnd;
        state.title_a = title_str;
    } else {
        state.hwnd_b = hwnd;
        state.title_b = title_str;
    }
}

#[tauri::command]
fn get_slots() -> serde_json::Value {
    let state = STATE.lock().unwrap();
    serde_json::json!({
        "hwnd_a": state.hwnd_a,
        "title_a": state.title_a,
        "hwnd_b": state.hwnd_b,
        "title_b": state.title_b,
        "is_active": state.is_active,
    })
}

#[tauri::command]
fn set_active(active: bool) {
    let mut state = STATE.lock().unwrap();
    state.is_active = active;
}

#[tauri::command]
fn minimize_window(window: tauri::Window) {
    let _ = window.minimize();
}

#[tauri::command]
fn close_window(window: tauri::Window) {
    let _ = window.close();
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .setup(|app| {
            let main_window = app.get_webview_window("main").unwrap();
            let raw_hwnd = main_window.hwnd().unwrap().0 as isize;
            
            *APP_HANDLE.lock().unwrap() = Some(app.handle().clone());
            
            // Set up main window configurations
            let _ = main_window.set_decorations(false); // frameless
            let _ = main_window.set_always_on_top(true);
            
            // Apply windows acrylic/mica effect using window-vibrancy
            #[cfg(target_os = "windows")]
            {
                if let Err(_) = window_vibrancy::apply_mica(&main_window, None) {
                    let _ = window_vibrancy::apply_acrylic(&main_window, Some((7, 8, 10, 180)));
                }
            }
            
            // Store main window HWND globally
            STATE.lock().unwrap().main_window_hwnd = Some(raw_hwnd);
            
            unsafe {
                // Register clipboard formats listener
                AddClipboardFormatListener(raw_hwnd);
                
                // Register global hotkey for Ctrl+Shift+C (ID = 101)
                RegisterHotKey(raw_hwnd, 101, MOD_CONTROL | MOD_SHIFT, 0x43);
                
                // Subclass main window WndProc
                let old_proc = SetWindowLongPtrW(raw_hwnd, GWLP_WNDPROC, subclass_wndproc as *const () as isize);
                ORIGINAL_WNDPROC = std::mem::transmute(old_proc);
            }
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            start_window_drag,
            start_drag_detect,
            set_slot,
            get_slots,
            set_active,
            minimize_window,
            close_window
        ])

        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
