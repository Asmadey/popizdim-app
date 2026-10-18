//! Keyboard focus around the file picker.
//!
//! Popizdim runs as an accessory (tray) app, so clicking the tray menu does
//! not activate it. Two consequences for the picker:
//! - the open panel would appear *behind* the app the user is in, so
//!   Popizdim activates itself first ([`activate_self`]);
//! - once the panel closes, focus does not go back by itself, and the
//!   transcript would later be pasted into the wrong place (or nowhere). The
//!   frontmost app is captured before the panel opens and handed activation
//!   back afterwards ([`FrontmostApp::restore`]).
//!
//! Since macOS 14 activation is cooperative: the active app has to yield to
//! the one it activates, so both calls run on the main thread while Popizdim
//! is the active app.

#[cfg(target_os = "macos")]
mod imp {
    use log::warn;
    use objc2::{available, MainThreadMarker};
    use objc2_app_kit::{
        NSApplication, NSApplicationActivationOptions, NSRunningApplication, NSWorkspace,
    };

    /// The app that was frontmost before Popizdim took focus. Only the
    /// process id is kept, so the value can cross threads.
    pub struct FrontmostApp {
        pid: i32,
    }

    impl FrontmostApp {
        /// `None` when Popizdim itself is frontmost (e.g. its settings
        /// window is open) or no app is.
        pub fn capture() -> Option<Self> {
            let app = NSWorkspace::sharedWorkspace().frontmostApplication()?;
            let pid = app.processIdentifier();
            (pid != std::process::id() as i32).then_some(Self { pid })
        }

        /// Hands activation back to the captured app. Main thread only.
        pub fn restore(&self) {
            let Some(mtm) = MainThreadMarker::new() else {
                warn!("Focus restore called off the main thread; skipped");
                return;
            };
            let Some(app) = NSRunningApplication::runningApplicationWithProcessIdentifier(self.pid)
            else {
                warn!("Previously focused app (pid {}) is gone", self.pid);
                return;
            };
            if available!(macos = 14.0) {
                NSApplication::sharedApplication(mtm).yieldActivationToApplication(&app);
            }
            if !app.activateWithOptions(NSApplicationActivationOptions::empty()) {
                warn!("Could not re-activate the previously focused app");
            }
        }
    }

    /// Brings Popizdim to the front so a panel it shows next is not hidden
    /// behind the current app. Main thread only.
    pub fn activate_self() {
        let Some(mtm) = MainThreadMarker::new() else {
            warn!("activate_self called off the main thread; skipped");
            return;
        };
        let app = NSApplication::sharedApplication(mtm);
        if available!(macos = 14.0) {
            app.activate();
        } else {
            #[allow(deprecated)]
            app.activateIgnoringOtherApps(true);
        }
    }
}

#[cfg(not(target_os = "macos"))]
mod imp {
    /// Focus handling is macOS-only for now.
    pub struct FrontmostApp;

    impl FrontmostApp {
        pub fn capture() -> Option<Self> {
            None
        }

        pub fn restore(&self) {}
    }

    pub fn activate_self() {}
}

pub use imp::{activate_self, FrontmostApp};
