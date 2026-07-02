use std::thread;
use std::time::Duration;
use windows_sys::Win32::Foundation::{HWND, POINT, TRUE, FALSE};
use windows_sys::Win32::UI::WindowsAndMessaging::{
    GetForegroundWindow, GetAncestor, WindowFromPoint, GetWindowTextW, IsWindow, ShowWindow, SetForegroundWindow, GA_ROOT, SW_RESTORE,
    SetWindowLongPtrW, GetWindowLongPtrW, SetLayeredWindowAttributes, GWL_EXSTYLE, WS_EX_LAYERED, LWA_ALPHA, GetWindowThreadProcessId
};
use windows_sys::Win32::System::Threading::{AttachThreadInput, GetCurrentThreadId};
use windows_sys::Win32::UI::Input::KeyboardAndMouse::{
    GetAsyncKeyState, keybd_event, VK_CONTROL, VK_SHIFT, VK_LSHIFT, VK_RSHIFT, KEYEVENTF_KEYUP, SetFocus
};
use windows_sys::Win32::System::DataExchange::{
    OpenClipboard, CloseClipboard, GetClipboardData, SetClipboardData, EmptyClipboard
};
use windows_sys::Win32::System::Memory::{GlobalAlloc, GlobalLock, GlobalUnlock, GHND};

// Native Win32 Clipboard helper to read CF_UNICODETEXT safely with retries
pub fn read_clipboard_text() -> Option<String> {
    for _ in 0..5 {
        unsafe {
            if OpenClipboard(0) != 0 {
                let handle = GetClipboardData(13); // CF_UNICODETEXT = 13
                if handle == 0 {
                    CloseClipboard();
                    return None;
                }
                let ptr = GlobalLock(handle as *mut std::ffi::c_void);
                if ptr.is_null() {
                    CloseClipboard();
                    return None;
                }
                
                // Calculate length of null-terminated UTF-16 string
                let mut len = 0;
                let mut p = ptr as *const u16;
                while *p != 0 {
                    len += 1;
                    p = p.add(1);
                }
                
                let slice = std::slice::from_raw_parts(ptr as *const u16, len);
                let text = String::from_utf16_lossy(slice);
                
                GlobalUnlock(handle as *mut std::ffi::c_void);
                CloseClipboard();
                return Some(text);
            }
        }
        thread::sleep(Duration::from_millis(15));
    }
    None
}

// Native Win32 Clipboard helper to write CF_UNICODETEXT safely with retries
pub fn write_clipboard_text(text: &str) -> bool {
    let wide: Vec<u16> = text.encode_utf16().chain(std::iter::once(0)).collect();
    let size = wide.len() * 2;
    
    for _ in 0..5 {
        unsafe {
            if OpenClipboard(0) != 0 {
                if EmptyClipboard() != 0 {
                    let handle = GlobalAlloc(GHND, size);
                    if handle.is_null() {
                        CloseClipboard();
                        return false;
                    }
                    let ptr = GlobalLock(handle);
                    if ptr.is_null() {
                        CloseClipboard();
                        return false;
                    }
                    std::ptr::copy_nonoverlapping(wide.as_ptr(), ptr as *mut u16, wide.len());
                    GlobalUnlock(handle);
                    
                    if SetClipboardData(13, handle as isize) != 0 { // CF_UNICODETEXT = 13
                        CloseClipboard();
                        return true;
                    }
                }
                CloseClipboard();
            }
        }
        thread::sleep(Duration::from_millis(15));
    }
    false
}

// Captures the top-level main window handle and title under the specified screen coordinates
pub fn get_window_under_cursor(x: i32, y: i32) -> Option<(HWND, String)> {
    unsafe {
        let pt = POINT { x, y };
        let hwnd_under_mouse = WindowFromPoint(pt);
        if hwnd_under_mouse == 0 {
            return None;
        }
        
        let root_hwnd = GetAncestor(hwnd_under_mouse, GA_ROOT);
        if root_hwnd == 0 {
            return None;
        }
        
        // Get window title
        let mut buf = [0u16; 512];
        let len = GetWindowTextW(root_hwnd, buf.as_mut_ptr(), buf.len() as i32);
        let title = if len > 0 {
            String::from_utf16_lossy(&buf[..len as usize])
        } else {
            "Untitled Window".to_string()
        };
        
        Some((root_hwnd, title))
    }
}

// Subclasses the window focus using AttachThreadInput for foreground switching bypass
pub fn force_foreground(hwnd: HWND) {
    unsafe {
        if IsWindow(hwnd) == 0 {
            return;
        }
        
        if windows_sys::Win32::UI::WindowsAndMessaging::IsIconic(hwnd) != 0 {
            ShowWindow(hwnd, SW_RESTORE);
        }
        
        // Bypass Windows ASST focus stealing protection by simulating an Alt keypress
        keybd_event(0x12, 0, 0, 0); // VK_MENU (Alt) Down
        keybd_event(0x12, 0, KEYEVENTF_KEYUP, 0); // VK_MENU (Alt) Up
        
        let fore_hwnd = GetForegroundWindow();
        let fore_thread = GetWindowThreadProcessId(fore_hwnd, std::ptr::null_mut());
        let target_thread = GetWindowThreadProcessId(hwnd, std::ptr::null_mut());
        let current_thread = GetCurrentThreadId();
        
        if fore_thread != target_thread {
            AttachThreadInput(fore_thread, target_thread, TRUE);
            SetForegroundWindow(hwnd);
            SetFocus(hwnd);
            AttachThreadInput(fore_thread, target_thread, FALSE);
        } else {
            SetForegroundWindow(hwnd);
            SetFocus(hwnd);
        }
        
        // Ensure active focus fallback
        if GetForegroundWindow() != hwnd {
            AttachThreadInput(current_thread, target_thread, TRUE);
            SetForegroundWindow(hwnd);
            SetFocus(hwnd);
            AttachThreadInput(current_thread, target_thread, FALSE);
        }
    }
}

// Sets the window opacity dynamically using Win32 Layered Window Attributes
pub fn set_window_opacity(hwnd: HWND, opacity: f32) {
    unsafe {
        let ex_style = GetWindowLongPtrW(hwnd, GWL_EXSTYLE);
        if (ex_style & WS_EX_LAYERED as isize) == 0 {
            SetWindowLongPtrW(hwnd, GWL_EXSTYLE, ex_style | WS_EX_LAYERED as isize);
        }
        let alpha = (opacity * 255.0) as u8;
        SetLayeredWindowAttributes(hwnd, 0, alpha, LWA_ALPHA);
    }
}


// Simulates a clean Ctrl+C copy without desynchronizing keyboard modifiers
pub fn send_copy() {
    unsafe {
        let ctrl_pressed = GetAsyncKeyState(VK_CONTROL as i32) < 0;
        let lshift_pressed = GetAsyncKeyState(VK_LSHIFT as i32) < 0;
        let rshift_pressed = GetAsyncKeyState(VK_RSHIFT as i32) < 0;
        let shift_pressed = lshift_pressed || rshift_pressed;
        
        // Release Shift virtually to ensure clean Ctrl+C
        if lshift_pressed {
            keybd_event(VK_LSHIFT as u8, 0, KEYEVENTF_KEYUP, 0);
        }
        if rshift_pressed {
            keybd_event(VK_RSHIFT as u8, 0, KEYEVENTF_KEYUP, 0);
        }
        if shift_pressed {
            keybd_event(VK_SHIFT as u8, 0, KEYEVENTF_KEYUP, 0);
            thread::sleep(Duration::from_millis(20));
        }
        
        if ctrl_pressed {
            keybd_event(0x43, 0, 0, 0); // C down
            thread::sleep(Duration::from_millis(10));
            keybd_event(0x43, 0, KEYEVENTF_KEYUP, 0); // C up
        } else {
            keybd_event(VK_CONTROL as u8, 0, 0, 0);
            thread::sleep(Duration::from_millis(10));
            keybd_event(0x43, 0, 0, 0);
            thread::sleep(Duration::from_millis(10));
            keybd_event(0x43, 0, KEYEVENTF_KEYUP, 0);
            thread::sleep(Duration::from_millis(10));
            keybd_event(VK_CONTROL as u8, 0, KEYEVENTF_KEYUP, 0);
        }
        
        // Restore Shift virtually
        if lshift_pressed {
            keybd_event(VK_LSHIFT as u8, 0, 0, 0);
        }
        if rshift_pressed {
            keybd_event(VK_RSHIFT as u8, 0, 0, 0);
        }
        if shift_pressed {
            keybd_event(VK_SHIFT as u8, 0, 0, 0);
        }
    }
}

// Simulates a clean Ctrl+V paste without desynchronizing keyboard modifiers
pub fn send_paste() {
    unsafe {
        let ctrl_pressed = GetAsyncKeyState(VK_CONTROL as i32) < 0;
        let lshift_pressed = GetAsyncKeyState(VK_LSHIFT as i32) < 0;
        let rshift_pressed = GetAsyncKeyState(VK_RSHIFT as i32) < 0;
        let shift_pressed = lshift_pressed || rshift_pressed;
        
        // Release Shift virtually to ensure clean Ctrl+V paste
        if lshift_pressed {
            keybd_event(VK_LSHIFT as u8, 0, KEYEVENTF_KEYUP, 0);
        }
        if rshift_pressed {
            keybd_event(VK_RSHIFT as u8, 0, KEYEVENTF_KEYUP, 0);
        }
        if shift_pressed {
            keybd_event(VK_SHIFT as u8, 0, KEYEVENTF_KEYUP, 0);
            thread::sleep(Duration::from_millis(20));
        }
        
        if ctrl_pressed {
            keybd_event(0x56, 0, 0, 0); // V down
            thread::sleep(Duration::from_millis(10));
            keybd_event(0x56, 0, KEYEVENTF_KEYUP, 0); // V up
        } else {
            keybd_event(VK_CONTROL as u8, 0, 0, 0);
            thread::sleep(Duration::from_millis(10));
            keybd_event(0x56, 0, 0, 0);
            thread::sleep(Duration::from_millis(10));
            keybd_event(0x56, 0, KEYEVENTF_KEYUP, 0);
            thread::sleep(Duration::from_millis(10));
            keybd_event(VK_CONTROL as u8, 0, KEYEVENTF_KEYUP, 0);
        }
        
        // Restore Shift virtually
        if lshift_pressed {
            keybd_event(VK_LSHIFT as u8, 0, 0, 0);
        }
        if rshift_pressed {
            keybd_event(VK_RSHIFT as u8, 0, 0, 0);
        }
        if shift_pressed {
            keybd_event(VK_SHIFT as u8, 0, 0, 0);
        }
    }
}
