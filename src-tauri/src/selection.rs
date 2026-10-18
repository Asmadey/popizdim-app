//! Reads the text selected in the frontmost app through the macOS
//! Accessibility API, without touching the clipboard.
//!
//! The focused element of the focused app is asked for `AXSelectedText`;
//! web views (Safari, WebKit) answer through text markers instead, and the
//! selection may live on an ancestor (the web area), so the lookup walks up a
//! few levels. Chromium and Electron apps build their accessibility tree only
//! for assistive apps: when they return nothing, `AXManualAccessibility` (and,
//! for Chromium browsers, `AXEnhancedUserInterface`) is switched on for that
//! app and the lookup is retried. Apps that expose no
//! selection at all (some PDF viewers, terminals, Java apps) yield `None`.
//!
//! Requires the Accessibility permission, which Popizdim already needs to
//! paste text.

use std::fmt;

/// Why the selection could not be read.
#[derive(Debug, PartialEq, Eq)]
pub enum SelectionError {
    /// Popizdim lacks the Accessibility permission.
    NotTrusted,
    /// No app has keyboard focus.
    NoFocusedApp,
    /// Not available on this platform. Only built off macOS.
    #[cfg_attr(target_os = "macos", allow(dead_code))]
    Unsupported,
}

impl fmt::Display for SelectionError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            SelectionError::NotTrusted => write!(
                f,
                "Popizdim needs the Accessibility permission to read selected text"
            ),
            SelectionError::NoFocusedApp => write!(f, "no app has keyboard focus"),
            SelectionError::Unsupported => {
                write!(f, "reading selected text is only available on macOS")
            }
        }
    }
}

/// The frontmost app's selected text; `Ok(None)` when nothing is selected or
/// the app doesn't expose its selection. Blocks for up to ~0.5 s while a
/// Chromium/Electron app builds its accessibility tree.
pub fn selected_text() -> Result<Option<String>, SelectionError> {
    imp::selected_text()
}

/// Treats whitespace-only selections as no selection.
fn meaningful(text: String) -> Option<String> {
    (!text.trim().is_empty()).then_some(text)
}

#[cfg(target_os = "macos")]
mod imp {
    use super::{meaningful, SelectionError};
    use core_foundation::base::{CFType, CFTypeID, CFTypeRef, TCFType};
    use core_foundation::boolean::CFBoolean;
    use core_foundation::string::{CFString, CFStringRef};
    use log::debug;
    use std::ffi::c_void;
    use std::time::Duration;

    type AXUIElementRef = *const c_void;
    type AXError = i32;
    const AX_SUCCESS: AXError = 0;

    #[link(name = "ApplicationServices", kind = "framework")]
    extern "C" {
        fn AXIsProcessTrusted() -> u8;
        fn AXUIElementCreateSystemWide() -> AXUIElementRef;
        fn AXUIElementCreateApplication(pid: i32) -> AXUIElementRef;
        fn AXUIElementGetTypeID() -> CFTypeID;
        fn AXUIElementCopyAttributeValue(
            element: AXUIElementRef,
            attribute: CFStringRef,
            value: *mut CFTypeRef,
        ) -> AXError;
        fn AXUIElementCopyParameterizedAttributeValue(
            element: AXUIElementRef,
            attribute: CFStringRef,
            parameter: CFTypeRef,
            result: *mut CFTypeRef,
        ) -> AXError;
        fn AXUIElementSetAttributeValue(
            element: AXUIElementRef,
            attribute: CFStringRef,
            value: CFTypeRef,
        ) -> AXError;
        fn AXUIElementSetMessagingTimeout(element: AXUIElementRef, seconds: f32) -> AXError;
    }

    /// Seconds an unresponsive app may take to answer one AX call.
    const MESSAGING_TIMEOUT_SECS: f32 = 0.25;

    /// Ancestors searched for a selection above the focused element.
    const MAX_ANCESTORS: usize = 8;

    /// Retries after enabling accessibility in a Chromium/Electron app. The
    /// tree builds asynchronously; Claude (Electron) needed more than 0.4 s.
    const RETRIES: usize = 10;
    const RETRY_DELAY: Duration = Duration::from_millis(100);

    /// An owned `AXUIElementRef` (released on drop by `CFType`).
    struct Element(CFType);

    impl Element {
        fn raw(&self) -> AXUIElementRef {
            self.0.as_CFTypeRef()
        }

        fn copy_attribute(&self, name: &str) -> Option<CFType> {
            self.try_copy_attribute(name).ok()
        }

        /// Like [`Self::copy_attribute`], keeping the AX error code (a null
        /// value on success reads as `AX_SUCCESS`).
        fn try_copy_attribute(&self, name: &str) -> Result<CFType, AXError> {
            let name = CFString::new(name);
            let mut value: CFTypeRef = std::ptr::null();
            // SAFETY: `self` is a live AXUIElement, `name` a live CFString and
            // `value` a valid out-pointer. On success the caller owns the
            // returned +1 reference, which `wrap_under_create_rule` adopts.
            let err = unsafe {
                AXUIElementCopyAttributeValue(self.raw(), name.as_concrete_TypeRef(), &mut value)
            };
            if err != AX_SUCCESS || value.is_null() {
                return Err(err);
            }
            // SAFETY: see above; `value` is non-null and owned by us.
            Ok(unsafe { CFType::wrap_under_create_rule(value) })
        }

        fn string(&self, name: &str) -> Option<String> {
            self.copy_attribute(name)?
                .downcast::<CFString>()
                .map(|s| s.to_string())
        }

        fn element(&self, name: &str) -> Option<Element> {
            let value = self.copy_attribute(name)?;
            // SAFETY: plain type-id query.
            (value.type_of() == unsafe { AXUIElementGetTypeID() }).then_some(Element(value))
        }

        fn parameterized_string(&self, name: &str, parameter: &CFType) -> Option<String> {
            let name = CFString::new(name);
            let mut value: CFTypeRef = std::ptr::null();
            // SAFETY: as in `copy_attribute`; `parameter` is a live CF object.
            let err = unsafe {
                AXUIElementCopyParameterizedAttributeValue(
                    self.raw(),
                    name.as_concrete_TypeRef(),
                    parameter.as_CFTypeRef(),
                    &mut value,
                )
            };
            if err != AX_SUCCESS || value.is_null() {
                return None;
            }
            // SAFETY: non-null +1 reference owned by us.
            unsafe { CFType::wrap_under_create_rule(value) }
                .downcast::<CFString>()
                .map(|s| s.to_string())
        }

        /// Sets a boolean attribute to true and returns the AX error code.
        fn set_true(&self, name: &str) -> AXError {
            let name = CFString::new(name);
            // SAFETY: live element, attribute name and CFBoolean.
            unsafe {
                AXUIElementSetAttributeValue(
                    self.raw(),
                    name.as_concrete_TypeRef(),
                    CFBoolean::true_value().as_CFTypeRef(),
                )
            }
        }

        fn set_timeout(&self, seconds: f32) {
            // SAFETY: live element.
            unsafe { AXUIElementSetMessagingTimeout(self.raw(), seconds) };
        }

        /// This element's selection, or the nearest ancestor's.
        fn selection(self) -> Option<String> {
            let mut current = self;
            for _ in 0..=MAX_ANCESTORS {
                if let Some(text) = current.string("AXSelectedText").and_then(meaningful) {
                    return Some(text);
                }
                // WebKit/Chromium web content: selection as a text-marker range.
                if let Some(range) = current.copy_attribute("AXSelectedTextMarkerRange") {
                    if let Some(text) = current
                        .parameterized_string("AXStringForTextMarkerRange", &range)
                        .and_then(meaningful)
                    {
                        return Some(text);
                    }
                }
                if matches!(
                    current.string("AXRole").as_deref(),
                    Some("AXWindow" | "AXApplication")
                ) {
                    return None;
                }
                current = current.element("AXParent")?;
            }
            None
        }
    }

    /// The app with keyboard focus. The system-wide `AXFocusedApplication`
    /// query can fail even with the permission granted, so fall back to the
    /// frontmost app reported by NSWorkspace.
    fn focused_app() -> Option<Element> {
        // SAFETY: returns a new +1 AXUIElement, adopted by `CFType`.
        let system =
            Element(unsafe { CFType::wrap_under_create_rule(AXUIElementCreateSystemWide()) });
        system.set_timeout(MESSAGING_TIMEOUT_SECS);
        match system.try_copy_attribute("AXFocusedApplication") {
            // SAFETY: plain type-id query.
            Ok(value) if value.type_of() == unsafe { AXUIElementGetTypeID() } => {
                return Some(Element(value));
            }
            Ok(_) => debug!("AXFocusedApplication is not an AXUIElement"),
            Err(err) => debug!("AXFocusedApplication failed with AX error {}", err),
        }
        let pid = objc2_app_kit::NSWorkspace::sharedWorkspace()
            .frontmostApplication()?
            .processIdentifier();
        debug!("Falling back to the frontmost app (pid {})", pid);
        // SAFETY: returns a new +1 AXUIElement for `pid`, adopted by `CFType`.
        Some(Element(unsafe {
            CFType::wrap_under_create_rule(AXUIElementCreateApplication(pid))
        }))
    }

    /// Whether the frontmost app is a Chromium-based browser, which builds its
    /// accessibility tree on `AXEnhancedUserInterface` as well as on
    /// `AXManualAccessibility`.
    fn chromium_browser_in_front() -> bool {
        objc2_app_kit::NSWorkspace::sharedWorkspace()
            .frontmostApplication()
            .and_then(|app| app.bundleIdentifier())
            .map(|id| {
                let id = id.to_string().to_lowercase();
                [
                    "chrome",
                    "chromium",
                    "brave",
                    "edgemac",
                    "vivaldi",
                    "opera",
                    "thebrowser",
                ]
                .iter()
                .any(|name| id.contains(name))
            })
            .unwrap_or(false)
    }

    pub fn selected_text() -> Result<Option<String>, SelectionError> {
        // SAFETY: no arguments; reads this process's TCC state.
        if unsafe { AXIsProcessTrusted() } == 0 {
            return Err(SelectionError::NotTrusted);
        }
        let app = focused_app().ok_or(SelectionError::NoFocusedApp)?;
        app.set_timeout(MESSAGING_TIMEOUT_SECS);

        let read = || match app.try_copy_attribute("AXFocusedUIElement") {
            // SAFETY: plain type-id query.
            Ok(value) if value.type_of() == unsafe { AXUIElementGetTypeID() } => {
                Element(value).selection()
            }
            Ok(_) => {
                debug!("AXFocusedUIElement is not an AXUIElement");
                None
            }
            Err(err) => {
                debug!("AXFocusedUIElement failed with AX error {}", err);
                None
            }
        };
        if let Some(text) = read() {
            return Ok(Some(text));
        }
        // Chromium/Electron: ask the app to build its accessibility tree.
        // Other apps reject the attribute, and then there is nothing to retry.
        let manual = app.set_true("AXManualAccessibility");
        let mut enabled = manual == AX_SUCCESS;
        if !enabled && chromium_browser_in_front() {
            let enhanced = app.set_true("AXEnhancedUserInterface");
            debug!(
                "AXManualAccessibility failed with AX error {}; AXEnhancedUserInterface: {}",
                manual, enhanced
            );
            enabled = enhanced == AX_SUCCESS;
        }
        if !enabled {
            debug!(
                "No selection in the focused app, and it did not accept AXManualAccessibility (AX error {})",
                manual
            );
            return Ok(None);
        }
        debug!("Enabled accessibility in the focused app; retrying");
        for _ in 0..RETRIES {
            std::thread::sleep(RETRY_DELAY);
            if let Some(text) = read() {
                return Ok(Some(text));
            }
        }
        Ok(None)
    }
}

#[cfg(not(target_os = "macos"))]
mod imp {
    use super::SelectionError;

    pub fn selected_text() -> Result<Option<String>, SelectionError> {
        Err(SelectionError::Unsupported)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn whitespace_is_not_a_selection() {
        assert_eq!(meaningful("  \n\t".to_string()), None);
        assert_eq!(
            meaningful(" Привет ".to_string()),
            Some(" Привет ".to_string())
        );
    }

    #[test]
    fn errors_explain_the_fix() {
        assert!(SelectionError::NotTrusted
            .to_string()
            .contains("Accessibility permission"));
    }
}
