//! Standalone modifier taps, committed on release without consuming system input.
use std::sync::Mutex;

#[derive(Clone, Copy)]
struct Modifier {
    #[cfg(target_os = "macos")]
    key_code: u16,
    #[cfg(target_os = "macos")]
    mask: usize,
    #[cfg(target_os = "windows")]
    vk: u32,
}
fn modifier(name: &str) -> Option<Modifier> {
    let (key_code, mask, vk) = match name.to_ascii_uppercase().as_str() {
        "LCOMMAND" | "COMMAND" | "SUPER" => (0x37, 0x8, 0x5B),
        "RCOMMAND" | "RIGHTCOMMAND" => (0x36, 0x10, 0x5C),
        "LSHIFT" | "SHIFT" => (0x38, 0x2, 0xA0),
        "RSHIFT" => (0x3C, 0x4, 0xA1),
        "LCONTROL" | "CONTROL" | "CTRL" => (0x3B, 0x1, 0xA2),
        "RCONTROL" | "RIGHTCONTROL" => (0x3E, 0x2000, 0xA3),
        "LALT" | "ALT" | "OPTION" => (0x3A, 0x20, 0xA4),
        "RALT" => (0x3D, 0x40, 0xA5),
        "FN" if cfg!(target_os = "macos") => (0x3F, 0x800000, 0),
        _ => return None,
    };
    let _ = (key_code, mask, vk);
    Some(Modifier {
        #[cfg(target_os = "macos")]
        key_code,
        #[cfg(target_os = "macos")]
        mask,
        #[cfg(target_os = "windows")]
        vk,
    })
}
pub fn is_modifier(name: &str) -> bool {
    modifier(name).is_some()
}

#[derive(Default)]
struct Tap {
    enabled: bool,
    target: Option<Modifier>,
    down: bool,
    used: bool,
}
impl Tap {
    fn flags(&mut self, target_event: bool, down: bool, other_modifier: bool) -> bool {
        if !self.enabled {
            return false;
        }
        if !target_event {
            if self.down {
                self.used = true;
            }
            return false;
        }
        if down {
            if !self.down {
                self.used = other_modifier;
            }
            self.down = true;
            false
        } else {
            let fire = self.down && !self.used && !other_modifier;
            self.down = false;
            self.used = false;
            fire
        }
    }
    fn other_key(&mut self) {
        if self.down {
            self.used = true;
        }
    }
}
static TAP: Mutex<Tap> = Mutex::new(Tap {
    enabled: false,
    target: None,
    down: false,
    used: false,
});

pub fn configure(name: Option<&str>) -> Result<(), String> {
    if name.is_some() && !cfg!(any(target_os = "macos", target_os = "windows")) {
        return Err("此平台尚未实现单独修饰键监听".into());
    }
    let target = name.and_then(modifier);
    *TAP.lock().map_err(|e| e.to_string())? = Tap {
        enabled: target.is_some(),
        target,
        ..Tap::default()
    };
    Ok(())
}

#[cfg(target_os = "macos")]
mod native {
    use super::*;
    use block2::RcBlock;
    use objc2::{rc::Retained, runtime::AnyObject};
    use objc2_app_kit::{NSEvent, NSEventMask, NSEventType};
    use std::{
        cell::RefCell,
        ptr::NonNull,
        sync::atomic::{AtomicBool, Ordering},
    };
    use tauri::{AppHandle, Emitter};

    static INSTALLED: AtomicBool = AtomicBool::new(false);
    #[link(name = "ApplicationServices", kind = "framework")]
    extern "C" {
        fn AXIsProcessTrusted() -> bool;
    }

    struct Monitors(Vec<Retained<AnyObject>>);
    impl Drop for Monitors {
        fn drop(&mut self) {
            for monitor in &self.0 {
                // These are AppKit monitor tokens, removed on their owning thread.
                unsafe {
                    NSEvent::removeMonitor(monitor);
                }
            }
        }
    }
    thread_local! { static MONITORS: RefCell<Option<Monitors>> = const { RefCell::new(None) }; }

    fn handle(app: &AppHandle, event: &NSEvent) {
        let fire = {
            let mut tap = TAP.lock().unwrap();
            let Some(target) = tap.target else { return };
            if event.r#type() == NSEventType::FlagsChanged {
                // Device-dependent modifier bits distinguish the left/right keys.
                let flags = event.modifierFlags().bits();
                let other = flags & (0x206F | 0x10 | 0x800000) & !target.mask != 0;
                tap.flags(
                    event.keyCode() == target.key_code,
                    flags & target.mask != 0,
                    other,
                )
            } else {
                tap.other_key();
                false
            }
        };
        if fire {
            let _ = app.emit_to("main", "toggle-requested", ());
        }
    }

    pub fn install(app: &AppHandle) -> Result<(), String> {
        let mask = NSEventMask::FlagsChanged
            | NSEventMask::KeyDown
            | NSEventMask::KeyUp
            | NSEventMask::LeftMouseDown
            | NSEventMask::RightMouseDown
            | NSEventMask::OtherMouseDown;
        let app_global = app.clone();
        let global = RcBlock::new(move |event: NonNull<NSEvent>| {
            // AppKit supplies a live event for the duration of this callback.
            handle(&app_global, unsafe { event.as_ref() });
        });
        let global = NSEvent::addGlobalMonitorForEventsMatchingMask_handler(mask, &global)
            .ok_or("无法安装单独修饰键全局监听")?;
        let mut monitors = Monitors(vec![global]);
        let app_local = app.clone();
        let local = RcBlock::new(move |event: NonNull<NSEvent>| {
            handle(&app_local, unsafe { event.as_ref() });
            event.as_ptr()
        });
        // Return the original event unmodified; never suppress keyboard input.
        let local = unsafe { NSEvent::addLocalMonitorForEventsMatchingMask_handler(mask, &local) }
            .ok_or("无法安装单独修饰键本地监听")?;
        monitors.0.push(local);
        MONITORS.with(|slot| *slot.borrow_mut() = Some(monitors));
        INSTALLED.store(true, Ordering::Relaxed);
        Ok(())
    }

    pub fn warning() -> Option<String> {
        if !INSTALLED.load(Ordering::Relaxed) {
            Some("单独修饰键监听安装失败，请重启应用或设置组合快捷键".into())
        } else if !unsafe { AXIsProcessTrusted() } {
            Some("单独修饰键全局监听需要辅助功能权限：请在系统设置 → 隐私与安全性 → 辅助功能中允许本应用，然后重启应用。".into())
        } else {
            None
        }
    }
}
#[cfg(any(target_os = "macos", target_os = "windows"))]
pub use native::{install, warning};

#[cfg(target_os = "windows")]
mod native {
    use super::*;
    use std::sync::OnceLock;
    use tauri::{AppHandle, Emitter};
    use windows_sys::Win32::{
        Foundation::{LPARAM, LRESULT, WPARAM},
        System::LibraryLoader::GetModuleHandleW,
        UI::{Input::KeyboardAndMouse::GetAsyncKeyState, WindowsAndMessaging::*},
    };
    static APP: OnceLock<AppHandle> = OnceLock::new();
    static INSTALLED: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

    unsafe extern "system" fn keyboard(code: i32, w: WPARAM, l: LPARAM) -> LRESULT {
        if code == HC_ACTION as i32 {
            let event = &*(l as *const KBDLLHOOKSTRUCT);
            let message = w as u32;
            if matches!(message, WM_KEYDOWN | WM_SYSKEYDOWN | WM_KEYUP | WM_SYSKEYUP)
                && event.flags & LLKHF_INJECTED == 0
            {
                let fire = {
                    let mut tap = TAP.lock().unwrap();
                    if let Some(target) = tap.target {
                        if event.vkCode == target.vk {
                            let other = [0x5B, 0x5C, 0xA0, 0xA1, 0xA2, 0xA3, 0xA4, 0xA5]
                                .iter()
                                .any(|&vk| vk != target.vk && GetAsyncKeyState(vk as i32) < 0);
                            tap.flags(true, matches!(message, WM_KEYDOWN | WM_SYSKEYDOWN), other)
                        } else {
                            tap.other_key();
                            false
                        }
                    } else {
                        false
                    }
                };
                if fire {
                    if let Some(app) = APP.get() {
                        let _ = app.emit_to("main", "toggle-requested", ());
                    }
                }
            }
        }
        CallNextHookEx(std::ptr::null_mut(), code, w, l)
    }
    unsafe extern "system" fn mouse(code: i32, w: WPARAM, l: LPARAM) -> LRESULT {
        if code == HC_ACTION as i32
            && matches!(w as u32, WM_LBUTTONDOWN | WM_RBUTTONDOWN | WM_MBUTTONDOWN)
        {
            TAP.lock().unwrap().other_key();
        }
        CallNextHookEx(std::ptr::null_mut(), code, w, l)
    }
    pub fn install(app: &AppHandle) -> Result<(), String> {
        let _ = APP.set(app.clone());
        let (tx, rx) = std::sync::mpsc::sync_channel(1);
        std::thread::spawn(move || unsafe {
            let module = GetModuleHandleW(std::ptr::null());
            let keyboard_hook = SetWindowsHookExW(WH_KEYBOARD_LL, Some(keyboard), module, 0);
            let mouse_hook = SetWindowsHookExW(WH_MOUSE_LL, Some(mouse), module, 0);
            if keyboard_hook.is_null() || mouse_hook.is_null() {
                if !keyboard_hook.is_null() {
                    UnhookWindowsHookEx(keyboard_hook);
                }
                if !mouse_hook.is_null() {
                    UnhookWindowsHookEx(mouse_hook);
                }
                let _ = tx.send(Err("无法安装单键修饰键监听".to_string()));
                return;
            }
            INSTALLED.store(true, std::sync::atomic::Ordering::Relaxed);
            let _ = tx.send(Ok(()));
            let mut message = std::mem::zeroed();
            while GetMessageW(&mut message, std::ptr::null_mut(), 0, 0) > 0 {
                TranslateMessage(&message);
                DispatchMessageW(&message);
            }
            UnhookWindowsHookEx(keyboard_hook);
            UnhookWindowsHookEx(mouse_hook);
        });
        rx.recv().map_err(|e| e.to_string())?
    }
    pub fn warning() -> Option<String> {
        (!INSTALLED.load(std::sync::atomic::Ordering::Relaxed))
            .then(|| "单键修饰键监听安装失败，请重启应用".into())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn tap() -> Tap {
        Tap {
            enabled: true,
            ..Tap::default()
        }
    }
    #[test]
    fn each_standalone_release_fires_once() {
        let mut t = tap();
        assert!(!t.flags(true, false, false)); // installed while already held
        for _ in 0..2 {
            assert!(!t.flags(true, true, false));
            assert!(!t.flags(true, true, false)); // held / duplicate event
            assert!(t.flags(true, false, false));
            assert!(!t.flags(true, false, false));
        }
    }
    #[test]
    fn combinations_and_other_modifiers_do_not_fire() {
        let mut t = tap();
        assert!(!t.flags(false, false, true));
        t.flags(true, true, false);
        t.other_key(); // Command+C, including autorepeat
        assert!(!t.flags(true, false, false));
        t.flags(true, true, true); // another modifier was already held
        assert!(!t.flags(true, false, false));
        t.flags(true, true, false);
        t.flags(false, true, true); // another modifier pressed afterwards
        assert!(!t.flags(true, false, false));
        assert!(!Tap::default().flags(true, true, false));
    }

    #[test]
    fn all_modifier_sides_have_distinct_bindings() {
        for (left, right) in [
            ("LCommand", "RCommand"),
            ("LControl", "RControl"),
            ("LShift", "RShift"),
            ("LAlt", "RAlt"),
        ] {
            let left = modifier(left).unwrap();
            let right = modifier(right).unwrap();
            #[cfg(target_os = "macos")]
            {
                assert_ne!(left.key_code, right.key_code);
                assert_eq!(left.mask & right.mask, 0);
            }
            #[cfg(target_os = "windows")]
            assert_ne!(left.vk, right.vk);
            let _ = (left, right);
        }
        assert!(!is_modifier("Control+Shift+Space"));
    }
}
