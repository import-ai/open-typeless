//! A standalone right-Command tap, committed on release. AppKit monitors never
//! consume events, so normal Command combinations continue to reach other apps.
use std::sync::Mutex;

#[derive(Default)]
struct Tap {
    enabled: bool,
    down: bool,
    used: bool,
}
impl Tap {
    fn flags(&mut self, right_event: bool, down: bool, other_modifier: bool) -> bool {
        if !self.enabled {
            return false;
        }
        if !right_event {
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
    down: false,
    used: false,
});

pub fn enable(enabled: bool) {
    *TAP.lock().unwrap() = Tap {
        enabled,
        ..Tap::default()
    };
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
            if event.r#type() == NSEventType::FlagsChanged {
                // SDK: kVK_RightCommand; NX_DEVICERCMDKEYMASK. Preserve the
                // device-dependent bits: the generic Command flag merges sides.
                let flags = event.modifierFlags().bits();
                let other = flags & (0x8 | 0x20000 | 0x40000 | 0x80000 | 0x800000) != 0;
                tap.flags(event.keyCode() == 0x36, flags & 0x10 != 0, other)
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
            .ok_or("无法安装右 Command 全局监听")?;
        let mut monitors = Monitors(vec![global]);
        let app_local = app.clone();
        let local = RcBlock::new(move |event: NonNull<NSEvent>| {
            handle(&app_local, unsafe { event.as_ref() });
            event.as_ptr()
        });
        // Return the original event unmodified; never suppress keyboard input.
        let local = unsafe { NSEvent::addLocalMonitorForEventsMatchingMask_handler(mask, &local) }
            .ok_or("无法安装右 Command 本地监听")?;
        monitors.0.push(local);
        MONITORS.with(|slot| *slot.borrow_mut() = Some(monitors));
        INSTALLED.store(true, Ordering::Relaxed);
        Ok(())
    }

    pub fn warning() -> Option<String> {
        if !INSTALLED.load(Ordering::Relaxed) {
            Some("右 Command 监听安装失败，请重启应用或设置组合快捷键".into())
        } else if !unsafe { AXIsProcessTrusted() } {
            Some("右 Command 全局监听需要辅助功能权限：请在系统设置 → 隐私与安全性 → 辅助功能中允许本应用，然后重启应用。".into())
        } else {
            None
        }
    }
}
#[cfg(target_os = "macos")]
pub use native::{install, warning};

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
    fn combinations_and_left_command_do_not_fire() {
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
}
