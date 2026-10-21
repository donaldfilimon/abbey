//! Native macOS window appearance.
//!
//! The desktop client is an opaque reading app: the webview paints its own
//! background and the reading surfaces are never translucent. That is the
//! right default for a reading surface, but it also means the window *frame*
//! is a flat rectangle of colour instead of the platform's own material.
//!
//! This module installs real AppKit material into the window's titlebar
//! strip — `NSGlassEffectView` (the Liquid Glass material) on the systems
//! that have it, `NSVisualEffectView` with the titlebar material before that
//! — and leaves the webview content area completely untouched and opaque.
//!
//! The rules this module keeps:
//!
//! * **macOS only.** Every other target compiles this module down to a no-op
//!   that reports an opaque window.
//! * **The webview is never made transparent.** The Tauri window stays
//!   `transparent: false`, the webview keeps painting its own opaque
//!   background, and the content area keeps an opaque safety panel behind
//!   the webview, so the window is never a see-through hole. Only the
//!   frame's titlebar strip — the part of the window AppKit reserves for the
//!   titlebar and the window buttons, above the content — is a material
//!   surface.
//! * **Degrade, never half-build.** Reduced transparency, increased contrast,
//!   forced colours, a window without a titlebar, or any failure while
//!   constructing the effect view all end as a fully opaque window, and a
//!   previously installed effect view is removed again.
//! * **No private API, no entitlement, no `LSUIElement`, no network.** Only
//!   documented AppKit classes and selectors are used, and the module reads
//!   nothing but the window it is given and the macOS accessibility display
//!   options.

use std::fmt;

/// What the window frame ended up being.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AppearanceKind {
    /// Real AppKit material is installed in the window frame.
    NativeMaterial,
    /// The window is fully opaque; no material is installed.
    Opaque,
}

impl AppearanceKind {
    /// A stable, short label for logs and diagnostics.
    pub fn label(self) -> &'static str {
        match self {
            Self::NativeMaterial => "native-material",
            Self::Opaque => "opaque",
        }
    }

    /// A one-line description of this kind.
    pub fn describe(self) -> String {
        match self {
            Self::NativeMaterial => "native material".to_owned(),
            Self::Opaque => "native material unavailable: opaque".to_owned(),
        }
    }
}

/// The native material that can be installed in the window frame.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NativeMaterial {
    /// `NSGlassEffectView` — the Liquid Glass material (macOS 26 and newer).
    LiquidGlass,
    /// `NSVisualEffectView` with the titlebar material (macOS 10.10 and
    /// newer).
    TitlebarVisualEffect,
}

impl NativeMaterial {
    /// A stable, short label for logs and diagnostics.
    pub fn label(self) -> &'static str {
        match self {
            Self::LiquidGlass => "liquid-glass",
            Self::TitlebarVisualEffect => "titlebar-visual-effect",
        }
    }

    /// A one-line description naming the AppKit class that provides it.
    pub fn describe(self) -> String {
        match self {
            Self::LiquidGlass => "liquid glass (AppKit NSGlassEffectView, macOS 26+)".to_owned(),
            Self::TitlebarVisualEffect => {
                "titlebar visual effect (AppKit NSVisualEffectView)".to_owned()
            }
        }
    }
}

/// Why the window stayed opaque.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OpaqueReason {
    /// Not macOS: there is no native window material to install.
    ///
    /// Only ever constructed on a non-macOS target, so it is unreachable in a
    /// macOS build.
    #[cfg_attr(target_os = "macos", allow(dead_code))]
    UnsupportedPlatform,
    /// The user asked macOS to reduce transparency.
    ReducedTransparency,
    /// The user asked macOS for increased contrast.
    IncreasedContrast,
    /// The user asked macOS to differentiate without colour, or to invert
    /// colours.
    ForcedColors,
    /// The window has no titlebar strip that could host material.
    NoTitlebar,
    /// AppKit did not hand over the native window.
    AppKitUnavailable,
    /// The effect view could not be constructed or installed.
    MaterialUnavailable,
    /// The call did not happen on the main thread, where AppKit requires it.
    NotMainThread,
}

impl OpaqueReason {
    /// A stable, short label for logs and diagnostics.
    pub fn label(self) -> &'static str {
        match self {
            Self::UnsupportedPlatform => "unsupported platform",
            Self::ReducedTransparency => "reduced transparency",
            Self::IncreasedContrast => "increased contrast",
            Self::ForcedColors => "forced colors",
            Self::NoTitlebar => "no titlebar",
            Self::AppKitUnavailable => "native window unavailable",
            Self::MaterialUnavailable => "material unavailable",
            Self::NotMainThread => "not on the main thread",
        }
    }
}

/// The bounded outcome of applying the native appearance.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Appearance {
    /// Native material is installed in the window frame.
    NativeMaterial(NativeMaterial),
    /// The window is fully opaque; no material is installed.
    Opaque(OpaqueReason),
}

impl Appearance {
    /// The coarse kind of this outcome.
    pub fn kind(self) -> AppearanceKind {
        match self {
            Self::NativeMaterial(_) => AppearanceKind::NativeMaterial,
            Self::Opaque(_) => AppearanceKind::Opaque,
        }
    }

    /// The native material, when one is installed.
    pub fn material(self) -> Option<NativeMaterial> {
        match self {
            Self::NativeMaterial(material) => Some(material),
            Self::Opaque(_) => None,
        }
    }

    /// The reason the window stayed opaque, when it did.
    pub fn opaque_reason(self) -> Option<OpaqueReason> {
        match self {
            Self::NativeMaterial(_) => None,
            Self::Opaque(reason) => Some(reason),
        }
    }

    /// A one-line description for the doctor surface and for logs.
    pub fn describe(self) -> String {
        match self {
            Self::NativeMaterial(material) => format!(
                "{}: {} — window frame only, the webview content stays opaque",
                AppearanceKind::NativeMaterial.describe(),
                material.describe()
            ),
            Self::Opaque(reason) => {
                format!("{} ({})", AppearanceKind::Opaque.describe(), reason.label())
            }
        }
    }
}

impl fmt::Display for Appearance {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.describe())
    }
}

/// The macOS accessibility display options that decide whether material may
/// be installed at all.
///
/// These are the same conditions the webview material already honours in CSS
/// (`prefers-reduced-transparency`, `forced-colors`, `prefers-contrast`). The
/// native material has to honour them too, because a native effect view is
/// not covered by any stylesheet.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct DisplayOptions {
    /// "Reduce transparency" is on.
    pub reduce_transparency: bool,
    /// "Increase contrast" is on.
    pub increase_contrast: bool,
    /// "Differentiate without colour" is on.
    pub differentiate_without_color: bool,
    /// "Invert colours" is on.
    pub invert_colors: bool,
}

impl DisplayOptions {
    /// The reason material must not be installed, if any.
    ///
    /// The order is fixed and total: the first condition that holds is the
    /// reported reason, so the same set of options always produces the same
    /// answer.
    pub fn opaque_reason(self) -> Option<OpaqueReason> {
        if self.reduce_transparency {
            Some(OpaqueReason::ReducedTransparency)
        } else if self.increase_contrast {
            Some(OpaqueReason::IncreasedContrast)
        } else if self.differentiate_without_color || self.invert_colors {
            Some(OpaqueReason::ForcedColors)
        } else {
            None
        }
    }
}

/// The pure decision: given the display options and the material the running
/// system offers, what should the window be?
///
/// This is deliberately free of AppKit so that the rule that matters — that
/// accessibility settings degrade the window to opaque — is testable
/// everywhere and cannot drift away from the AppKit code that applies it.
pub fn decide(options: DisplayOptions, material: NativeMaterial) -> Appearance {
    match options.opaque_reason() {
        Some(reason) => Appearance::Opaque(reason),
        None => Appearance::NativeMaterial(material),
    }
}

/// The outcome on a platform that has no native window material at all.
///
/// Only ever called on a non-macOS target, so it is unreachable in a macOS
/// build.
#[cfg_attr(target_os = "macos", allow(dead_code))]
pub const fn unsupported() -> Appearance {
    Appearance::Opaque(OpaqueReason::UnsupportedPlatform)
}

/// A rectangle in the frame view's coordinate system.
#[derive(Debug, Clone, Copy, PartialEq)]
struct Rect {
    x: f64,
    y: f64,
    width: f64,
    height: f64,
}

/// The shortest titlebar strip worth hosting material in. A strip thinner
/// than this is not a titlebar; it is a rounding artefact.
const MIN_TITLEBAR_STRIP_HEIGHT: f64 = 1.0;

/// The titlebar strip of a window, in the frame view's coordinate system.
///
/// AppKit's window frame stacks the titlebar above the content view inside
/// one frame view, so the strip is whatever is left above the content layout
/// rect — the area the webview is given. `None` means there is no usable
/// strip: a borderless window, a full-screen window (whose content fills the
/// whole frame), or a degenerate layout.
fn titlebar_strip(frame: Rect, content: Rect) -> Option<Rect> {
    let strip_height = frame.height - (content.y + content.height);
    if !strip_height.is_finite() || strip_height <= MIN_TITLEBAR_STRIP_HEIGHT {
        return None;
    }
    Some(Rect {
        x: frame.x,
        y: content.y + content.height,
        width: frame.width,
        height: strip_height,
    })
}

/// Apply the platform's native window appearance to `window`.
///
/// Returns the bounded outcome. This never fails and never panics: any
/// problem degrades to a fully opaque window. Calling it again re-evaluates
/// the decision and is idempotent — an unchanged decision leaves the
/// installed material exactly where it is, so there is no flicker.
pub fn apply<R: tauri::Runtime>(window: &tauri::WebviewWindow<R>) -> Appearance {
    #[cfg(target_os = "macos")]
    {
        macos::apply(window)
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = window;
        unsupported()
    }
}

#[cfg(target_os = "macos")]
mod macos {
    //! The AppKit half of [`super::apply`].
    //!
    //! Everything here runs on the main thread, which is where AppKit
    //! requires it. The module keeps exactly one piece of state: the effect
    //! view it installed, so that a later re-evaluation can remove it again
    //!

    use super::{
        Appearance, DisplayOptions, NativeMaterial, OpaqueReason, Rect, decide, titlebar_strip,
    };
    use objc2::MainThreadMarker;
    use objc2::msg_send;
    use objc2::rc::Retained;
    use objc2_app_kit::{
        NSAppKitVersionNumber, NSAutoresizingMaskOptions, NSBox, NSBoxType, NSColor,
        NSGlassEffectView, NSGlassEffectViewStyle, NSView, NSVisualEffectBlendingMode,
        NSVisualEffectMaterial, NSVisualEffectState, NSVisualEffectView, NSWindow,
        NSWindowOrderingMode, NSWindowStyleMask, NSWorkspace,
    };
    use objc2_foundation::{NSPoint, NSRect, NSSize};
    use std::cell::RefCell;

    /// `NSAppKitVersionNumber` for macOS 26.0, the release that introduced
    /// `NSGlassEffectView` — the Liquid Glass material. Same constant
    /// `window-vibrancy` uses.
    const MIN_APPKIT_VERSION_LIQUID_GLASS: f64 = 2685.0;

    // The material view this process installed, if any.
    //
    // AppKit work is main-thread only, so the handle belongs to the thread
    // that owns the view rather than to a global.
    thread_local! {
        static INSTALLED: RefCell<Option<Installed>> = const { RefCell::new(None) };
    }

    /// An installed material view, the opaque safety panel behind the webview,
    /// and the window background that has to be restored when the material is
    /// removed again.
    #[derive(Clone)]
    struct Installed {
        view: Retained<NSView>,
        safety: Retained<NSView>,
        background: Retained<NSColor>,
        material: NativeMaterial,
    }

    #[allow(unsafe_code)]
    pub(super) fn apply<R: tauri::Runtime>(window: &tauri::WebviewWindow<R>) -> Appearance {
        // AppKit forbids touching the view hierarchy off the main thread.
        // Report the honest outcome and leave any installed material alone.
        let Some(mtm) = MainThreadMarker::new() else {
            return Appearance::Opaque(OpaqueReason::NotMainThread);
        };
        let Ok(raw) = window.ns_window() else {
            return Appearance::Opaque(OpaqueReason::AppKitUnavailable);
        };
        // SAFETY: `ns_window()` hands back the `NSWindow` that Tauri owns for
        // this window. It is a live, non-null Objective-C object that outlives
        // the call, and AppKit objects are only touched on the main thread,
        // which was checked above.
        let ns_window: &NSWindow = unsafe { &*raw.cast::<NSWindow>() };

        // SAFETY: `NSAppKitVersionNumber` is a read-only AppKit constant that
        // AppKit itself publishes; reading it cannot have side effects.
        let material = if unsafe { NSAppKitVersionNumber } >= MIN_APPKIT_VERSION_LIQUID_GLASS {
            NativeMaterial::LiquidGlass
        } else {
            NativeMaterial::TitlebarVisualEffect
        };

        match decide(display_options(), material) {
            Appearance::Opaque(reason) => {
                remove(ns_window);
                Appearance::Opaque(reason)
            }
            Appearance::NativeMaterial(material) => match install(mtm, ns_window, material) {
                Ok(()) => Appearance::NativeMaterial(material),
                Err(reason) => {
                    remove(ns_window);
                    Appearance::Opaque(reason)
                }
            },
        }
    }

    /// The macOS accessibility display options.
    ///
    /// These live in an `NSWorkspace` category, so they are read through the
    /// Objective-C runtime rather than through a generated binding. `BOOL` is
    /// a signed `char` on macOS and Objective-C encodes it as `c`, so every
    /// value is read as `i8` and compared against zero.
    #[allow(unsafe_code)]
    fn display_options() -> DisplayOptions {
        let workspace = NSWorkspace::sharedWorkspace();
        let workspace: &NSWorkspace = &workspace;
        // SAFETY: `NSWorkspace` implements these four readonly `BOOL`
        // properties (declared in its `NSWorkspaceAccessibilityDisplay`
        // category). Each message returns a `BOOL` in the `c` encoding, which
        // `i8` matches exactly.
        let reduce_transparency: i8 =
            unsafe { msg_send![workspace, accessibilityDisplayShouldReduceTransparency] };
        let increase_contrast: i8 =
            unsafe { msg_send![workspace, accessibilityDisplayShouldIncreaseContrast] };
        let differentiate_without_color: i8 = unsafe {
            msg_send![
                workspace,
                accessibilityDisplayShouldDifferentiateWithoutColor
            ]
        };
        let invert_colors: i8 =
            unsafe { msg_send![workspace, accessibilityDisplayShouldInvertColors] };
        DisplayOptions {
            reduce_transparency: reduce_transparency != 0,
            increase_contrast: increase_contrast != 0,
            differentiate_without_color: differentiate_without_color != 0,
            invert_colors: invert_colors != 0,
        }
    }

    /// Install the material into the window's titlebar strip.
    #[allow(unsafe_code)]
    fn install(
        mtm: MainThreadMarker,
        ns_window: &NSWindow,
        material: NativeMaterial,
    ) -> Result<(), OpaqueReason> {
        let style = ns_window.styleMask();
        // A window without a visible titlebar has no frame strip to host
        // material in.
        if !style.contains(NSWindowStyleMask::Titled) {
            return Err(OpaqueReason::NoTitlebar);
        }
        // In full screen the titlebar collapses into the content area, so the
        // strip would sit on top of the reading surface instead of behind the
        // frame.
        if style.contains(NSWindowStyleMask::FullScreen) {
            return Err(OpaqueReason::NoTitlebar);
        }

        let Some(content) = ns_window.contentView() else {
            return Err(OpaqueReason::AppKitUnavailable);
        };
        // SAFETY: the content view's superview is the window's frame view,
        // which AppKit owns for the lifetime of the window. The call only
        // borrows the object; the returned handle is retained by AppKit's own
        // ownership of the frame view.
        let Some(frame_view) = (unsafe { content.superview() }) else {
            return Err(OpaqueReason::AppKitUnavailable);
        };

        // The titlebar strip is the part of the frame the content layout does
        // not use. The content layout rect is the area the webview is given;
        // whatever is left above it is the frame's own strip, which is where
        // the titlebar and the window buttons live.
        let strip = match titlebar_strip(
            to_rect(frame_view.bounds()),
            to_rect(ns_window.contentLayoutRect()),
        ) {
            Some(strip) => strip,
            None => return Err(OpaqueReason::NoTitlebar),
        };

        // Already installed with the same material: leave it exactly where it
        // is so that a re-evaluation (the window regaining focus, say) does
        // not flicker the frame.
        if let Some(existing) = installed() {
            if existing.material == material && existing.view.window().is_some() {
                return Ok(());
            }
            remove(ns_window);
        }

        let strip_rect = NSRect::new(
            NSPoint::new(strip.x, strip.y),
            NSSize::new(strip.width, strip.height),
        );
        let effect: Retained<NSView> = match material {
            NativeMaterial::LiquidGlass => {
                let glass = NSGlassEffectView::initWithFrame(mtm.alloc(), strip_rect);
                glass.setStyle(NSGlassEffectViewStyle::Regular);
                // A flush bar, not a floating capsule: the strip is the
                // window's own titlebar area.
                glass.setCornerRadius(0.0);
                glass.setAutoresizingMask(autoresizing_mask());
                Retained::into_super(glass)
            }
            NativeMaterial::TitlebarVisualEffect => {
                let effect = NSVisualEffectView::initWithFrame(mtm.alloc(), strip_rect);
                effect.setMaterial(NSVisualEffectMaterial::Titlebar);
                effect.setBlendingMode(NSVisualEffectBlendingMode::BehindWindow);
                effect.setState(NSVisualEffectState::FollowsWindowActiveState);
                effect.setAutoresizingMask(autoresizing_mask());
                Retained::into_super(effect)
            }
        };

        // The safety layer: an opaque panel behind the webview that covers
        // exactly the content area. AppKit's material samples what is behind
        // the window, which needs the window to be non-opaque; this panel is
        // what keeps the window from ever becoming a see-through hole if the
        // webview ever fails to paint. The content area is therefore opaque
        // in every case, and only the frame's titlebar strip is a material
        // surface.
        let content_rect = content.frame();
        let safety = NSBox::initWithFrame(mtm.alloc(), content_rect);
        safety.setBoxType(NSBoxType::Custom);
        safety.setTransparent(false);
        safety.setBorderWidth(0.0);
        safety.setFillColor(&NSColor::windowBackgroundColor());
        safety.setAutoresizingMask(
            NSAutoresizingMaskOptions::ViewWidthSizable
                | NSAutoresizingMaskOptions::ViewHeightSizable,
        );
        frame_view.addSubview_positioned_relativeTo(
            safety.as_ref(),
            NSWindowOrderingMode::Below,
            Some(content.as_ref()),
        );

        // The window itself becomes non-opaque with a clear background so
        // that AppKit's material has something to sample. Every pixel is
        // still painted: the webview over the content area, the material over
        // the titlebar strip, and the opaque safety panel under the webview.
        let background = ns_window.backgroundColor();
        ns_window.setOpaque(false);
        let clear = NSColor::clearColor();
        ns_window.setBackgroundColor(Some(&clear));

        // The material is placed directly above the content view in the
        // frame's z-order and directly below the titlebar container that
        // holds the window buttons and the title. It therefore covers exactly
        // the frame's own strip — the area the flat system titlebar used to
        // paint over — and never the reading surfaces below it.
        frame_view.addSubview_positioned_relativeTo(
            &effect,
            NSWindowOrderingMode::Above,
            Some(content.as_ref()),
        );
        // The titlebar must stop painting its own background, otherwise the
        // material behind it would never be seen.
        ns_window.setTitlebarAppearsTransparent(true);

        if effect.window().is_none() {
            // The view did not make it into the window: undo the partial
            // installation rather than leaving a half-built frame.
            effect.removeFromSuperview();
            ns_window.setTitlebarAppearsTransparent(false);
            return Err(OpaqueReason::MaterialUnavailable);
        }

        INSTALLED.with(|slot| {
            *slot.borrow_mut() = Some(Installed {
                view: effect,
                safety: Retained::into_super(safety),
                background,
                material,
            });
        });
        Ok(())
    }

    /// Remove an installed material view and restore the opaque window.
    fn remove(ns_window: &NSWindow) {
        if let Some(existing) = installed() {
            existing.view.removeFromSuperview();
            existing.safety.removeFromSuperview();
            ns_window.setTitlebarAppearsTransparent(false);
            // The window goes back to being opaque with the background it had
            // before the material was installed.
            ns_window.setOpaque(true);
            ns_window.setBackgroundColor(Some(&existing.background));
            clear_installed();
        }
    }

    /// The installed material view, if any.
    fn installed() -> Option<Installed> {
        INSTALLED.with(|slot| slot.borrow().clone())
    }

    /// Forget the installed material view.
    fn clear_installed() {
        INSTALLED.with(|slot| {
            *slot.borrow_mut() = None;
        });
    }

    /// The material view tracks the window's width and keeps its top edge
    /// pinned, so it still covers the strip after a resize.
    fn autoresizing_mask() -> NSAutoresizingMaskOptions {
        NSAutoresizingMaskOptions::ViewWidthSizable | NSAutoresizingMaskOptions::ViewMinYMargin
    }

    /// Convert an AppKit rectangle into the plain rectangle the pure helpers
    /// work with.
    fn to_rect(rect: NSRect) -> Rect {
        Rect {
            x: rect.origin.x,
            y: rect.origin.y,
            width: rect.size.width,
            height: rect.size.height,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{
        Appearance, AppearanceKind, DisplayOptions, MIN_TITLEBAR_STRIP_HEIGHT, NativeMaterial,
        OpaqueReason, Rect, decide, titlebar_strip, unsupported,
    };

    #[test]
    fn no_display_option_keeps_the_native_material() {
        let outcome = decide(DisplayOptions::default(), NativeMaterial::LiquidGlass);
        assert_eq!(
            outcome,
            Appearance::NativeMaterial(NativeMaterial::LiquidGlass)
        );
        assert_eq!(outcome.kind(), AppearanceKind::NativeMaterial);
        assert_eq!(outcome.material(), Some(NativeMaterial::LiquidGlass));
        assert_eq!(outcome.opaque_reason(), None);
    }

    #[test]
    fn reduced_transparency_degrades_to_opaque() {
        let options = DisplayOptions {
            reduce_transparency: true,
            ..DisplayOptions::default()
        };
        let outcome = decide(options, NativeMaterial::LiquidGlass);
        assert_eq!(
            outcome,
            Appearance::Opaque(OpaqueReason::ReducedTransparency)
        );
        assert_eq!(outcome.kind(), AppearanceKind::Opaque);
        assert_eq!(outcome.material(), None);
    }

    #[test]
    fn increased_contrast_degrades_to_opaque() {
        let options = DisplayOptions {
            increase_contrast: true,
            ..DisplayOptions::default()
        };
        assert_eq!(
            decide(options, NativeMaterial::TitlebarVisualEffect),
            Appearance::Opaque(OpaqueReason::IncreasedContrast)
        );
    }

    #[test]
    fn forced_colors_degrade_to_opaque() {
        let differentiate = DisplayOptions {
            differentiate_without_color: true,
            ..DisplayOptions::default()
        };
        assert_eq!(
            decide(differentiate, NativeMaterial::LiquidGlass),
            Appearance::Opaque(OpaqueReason::ForcedColors)
        );
        let invert = DisplayOptions {
            invert_colors: true,
            ..DisplayOptions::default()
        };
        assert_eq!(
            decide(invert, NativeMaterial::LiquidGlass),
            Appearance::Opaque(OpaqueReason::ForcedColors)
        );
    }

    #[test]
    fn the_first_display_condition_wins_and_is_stable() {
        let options = DisplayOptions {
            reduce_transparency: true,
            increase_contrast: true,
            differentiate_without_color: true,
            invert_colors: true,
        };
        assert_eq!(
            options.opaque_reason(),
            Some(OpaqueReason::ReducedTransparency)
        );
        // The same options always produce the same answer.
        assert_eq!(options.opaque_reason(), options.opaque_reason());
    }

    #[test]
    fn the_opaque_description_says_so_plainly() {
        let opaque = Appearance::Opaque(OpaqueReason::ReducedTransparency);
        let described = opaque.describe();
        assert!(
            described.contains("native material unavailable: opaque"),
            "unexpected description: {described}"
        );
        assert!(described.contains("reduced transparency"));
    }

    #[test]
    fn the_native_description_names_the_appkit_class() {
        let glass = Appearance::NativeMaterial(NativeMaterial::LiquidGlass).describe();
        assert!(glass.contains("NSGlassEffectView"), "unexpected: {glass}");
        assert!(glass.contains("opaque"), "unexpected: {glass}");
        let effect = Appearance::NativeMaterial(NativeMaterial::TitlebarVisualEffect).describe();
        assert!(
            effect.contains("NSVisualEffectView"),
            "unexpected: {effect}"
        );
    }

    #[test]
    fn a_platform_without_native_material_is_opaque() {
        assert_eq!(
            unsupported(),
            Appearance::Opaque(OpaqueReason::UnsupportedPlatform)
        );
        assert_eq!(unsupported().kind(), AppearanceKind::Opaque);
    }

    #[test]
    fn the_titlebar_strip_is_what_is_left_above_the_content_view() {
        let frame = Rect {
            x: 0.0,
            y: 0.0,
            width: 1120.0,
            height: 760.0,
        };
        let content = Rect {
            x: 0.0,
            y: 0.0,
            width: 1120.0,
            height: 732.0,
        };
        let strip = titlebar_strip(frame, content).expect("a titlebar strip");
        assert_eq!(strip.y, 732.0);
        assert_eq!(strip.height, 28.0);
        assert_eq!(strip.width, 1120.0);
    }

    #[test]
    fn a_window_without_a_titlebar_has_no_strip() {
        let frame = Rect {
            x: 0.0,
            y: 0.0,
            width: 1120.0,
            height: 760.0,
        };
        // A full-size content view covers the whole frame.
        let content = frame;
        assert_eq!(titlebar_strip(frame, content), None);
        // A hairline is not a titlebar.
        let hairline = Rect {
            x: 0.0,
            y: 0.0,
            width: 1120.0,
            height: frame.height - MIN_TITLEBAR_STRIP_HEIGHT / 2.0,
        };
        assert_eq!(titlebar_strip(frame, hairline), None);
        // A degenerate layout never produces a strip.
        let broken = Rect {
            x: 0.0,
            y: f64::NAN,
            width: 1120.0,
            height: 760.0,
        };
        assert_eq!(titlebar_strip(frame, broken), None);
    }
}
