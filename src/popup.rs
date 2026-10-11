//! The small settings popup: a borderless WebView window that appears above
//! the tray icon, like the pop-ups of other tray apps.
//!
//! It loads the same local settings page that the browser uses, with
//! `&popup=1` so the page switches to its compact layout. The popup needs the
//! Microsoft WebView2 runtime (included in Windows 11 and most Windows 10
//! installs). If it can't be created, the tray falls back to the browser.

use tao::event_loop::EventLoopWindowTarget;
use tray_icon::Rect;

#[cfg(windows)]
const WIDTH: f64 = 420.0;
#[cfg(windows)]
const HEIGHT: f64 = 640.0;
/// Space kept between the popup, the tray icon and the screen edges.
#[cfg(windows)]
const GAP: f64 = 10.0;

#[cfg(windows)]
pub struct Popup {
    window: tao::window::Window,
    // Must stay alive for as long as the window is shown.
    _webview: wry::WebView,
    visible: bool,
    shown_at: std::time::Instant,
}

#[cfg(windows)]
impl Popup {
    /// Creates the window (hidden) and loads `url`. Returns `None` if the
    /// window or WebView2 couldn't be created.
    pub fn new(target: &EventLoopWindowTarget<()>, url: &str) -> Option<Self> {
        use tao::{
            dpi::LogicalSize, platform::windows::WindowBuilderExtWindows, window::WindowBuilder,
        };
        use wry::WebViewBuilder;

        // Keep WebView2's cache in the CraftPulse config folder instead of
        // next to the exe.
        if let Some(dir) = dirs::config_dir() {
            std::env::set_var(
                "WEBVIEW2_USER_DATA_FOLDER",
                dir.join("CraftPulse").join("webview"),
            );
        }

        let window = WindowBuilder::new()
            .with_title("CraftPulse")
            .with_inner_size(LogicalSize::new(WIDTH, HEIGHT))
            .with_decorations(false)
            .with_resizable(false)
            .with_always_on_top(true)
            .with_skip_taskbar(true)
            .with_visible(false)
            .build(target)
            .ok()?;
        let webview = WebViewBuilder::new()
            .with_url(url)
            .with_background_color((17, 19, 24, 255))
            .build(&window)
            .ok()?;
        Some(Self {
            window,
            _webview: webview,
            visible: false,
            shown_at: std::time::Instant::now(),
        })
    }

    pub fn is_visible(&self) -> bool {
        self.visible
    }

    /// Shows the popup next to `anchor` (the tray icon's rectangle). Without
    /// an anchor it sits in the bottom-right corner of the primary monitor.
    pub fn show(&mut self, anchor: Option<Rect>) {
        self.place(anchor);
        self.window.set_visible(true);
        self.window.set_focus();
        self.visible = true;
        self.shown_at = std::time::Instant::now();
    }

    /// True once another program's window has come to the front, meaning the
    /// user clicked away and the popup should close.
    ///
    /// This checks the foreground window's process instead of using the
    /// window's own focus events, because the web view inside the popup takes
    /// focus from its parent window and would make those events fire at once.
    pub fn lost_focus(&self) -> bool {
        use windows::Win32::UI::WindowsAndMessaging::{GetForegroundWindow, GetWindowThreadProcessId};

        // Give the popup time to take focus after it opens.
        if !self.visible || self.shown_at.elapsed() < std::time::Duration::from_millis(400) {
            return false;
        }
        let mut pid = 0;
        unsafe {
            let foreground = GetForegroundWindow();
            GetWindowThreadProcessId(foreground, Some(&mut pid));
        }
        // pid 0 means there is no foreground window right now (mid-switch).
        pid != 0 && pid != std::process::id()
    }

    pub fn hide(&mut self) {
        self.window.set_visible(false);
        self.visible = false;
    }

    fn place(&self, anchor: Option<Rect>) {
        use tao::dpi::{LogicalSize, PhysicalPosition};

        // The monitor that holds the tray icon, or the primary one.
        let monitor = anchor
            .as_ref()
            .and_then(|rect| {
                self.window.available_monitors().find(|monitor| {
                    let position = monitor.position();
                    let size = monitor.size();
                    let (x, y) = (rect.position.x, rect.position.y);
                    x >= position.x as f64
                        && x < position.x as f64 + size.width as f64
                        && y >= position.y as f64
                        && y < position.y as f64 + size.height as f64
                })
            })
            .or_else(|| self.window.primary_monitor());
        let Some(monitor) = monitor else {
            return;
        };

        let scale = monitor.scale_factor();
        let (mx, my) = (monitor.position().x as f64, monitor.position().y as f64);
        let (mw, mh) = (monitor.size().width as f64, monitor.size().height as f64);

        // Keep the popup on screen even on small displays.
        let height = HEIGHT.min(mh / scale - 120.0).max(360.0);
        self.window
            .set_inner_size(LogicalSize::new(WIDTH, height));
        let (w, h) = (WIDTH * scale, height * scale);
        let gap = GAP * scale;

        let (x, y) = match anchor {
            Some(rect) => {
                let icon_centre = rect.position.x + rect.size.width as f64 / 2.0;
                // Taskbar at the bottom: open upwards. At the top: open downwards.
                let taskbar_at_bottom = rect.position.y > my + mh / 2.0;
                let y = if taskbar_at_bottom {
                    rect.position.y - h - gap
                } else {
                    rect.position.y + rect.size.height as f64 + gap
                };
                (icon_centre - w / 2.0, y)
            }
            // No anchor (opened from the menu): bottom-right, above a typical taskbar.
            None => (mx + mw - w - gap, my + mh - h - gap - 48.0 * scale),
        };
        let x = x.clamp(mx + gap, (mx + mw - w - gap).max(mx + gap));
        let y = y.clamp(my + gap, (my + mh - h - gap).max(my + gap));
        self.window.set_outer_position(PhysicalPosition::new(x, y));
    }
}

/// Other platforms keep the old behaviour: the tray opens the browser.
#[cfg(not(windows))]
pub struct Popup;

#[cfg(not(windows))]
#[allow(dead_code)]
impl Popup {
    pub fn new(_target: &EventLoopWindowTarget<()>, _url: &str) -> Option<Self> {
        None
    }

    pub fn is_visible(&self) -> bool {
        false
    }

    pub fn show(&mut self, _anchor: Option<Rect>) {}

    pub fn hide(&mut self) {}

    pub fn lost_focus(&self) -> bool {
        false
    }
}
