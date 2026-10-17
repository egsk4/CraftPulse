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
    _context: wry::WebContext,
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
        let mut context = wry::WebContext::new(
            dirs::config_dir().map(|dir| dir.join("CraftPulse").join("webview")),
        );

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
        let webview = WebViewBuilder::new_with_web_context(&mut context)
            .with_url(url)
            .with_background_color((17, 19, 24, 255))
            .build(&window)
            .ok()?;
        Some(Self {
            window,
            _webview: webview,
            _context: context,
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
        use windows::Win32::UI::WindowsAndMessaging::{
            GetForegroundWindow, GetWindowThreadProcessId,
        };

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
        self.window.set_inner_size(LogicalSize::new(WIDTH, height));
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

/// A native settings window on Linux. Wayland controls window placement,
/// so keep normal decorations and let the compositor place it.
#[cfg(target_os = "linux")]
pub struct Popup {
    window: tao::window::Window,
    _webview: wry::WebView,
    visible: bool,
}

#[cfg(target_os = "linux")]
impl Popup {
    pub fn new(target: &EventLoopWindowTarget<()>, url: &str) -> Option<Self> {
        use tao::{dpi::LogicalSize, platform::unix::WindowExtUnix, window::WindowBuilder};
        use wry::WebViewBuilderExtUnix;
        let window = WindowBuilder::new()
            .with_title("CraftPulse")
            .with_inner_size(LogicalSize::new(420.0, 640.0))
            .with_visible(false)
            .build(target)
            .ok()?;
        let webview = wry::WebViewBuilder::new()
            .with_url(url)
            .build_gtk(window.default_vbox()?)
            .ok()?;
        Some(Self {
            window,
            _webview: webview,
            visible: false,
        })
    }

    pub fn is_visible(&self) -> bool {
        self.visible
    }
    pub fn show(&mut self, _anchor: Option<Rect>) {
        self.window.set_visible(true);
        self.window.set_focus();
        self.visible = true;
    }
    pub fn hide(&mut self) {
        self.window.set_visible(false);
        self.visible = false;
    }
    // Tiling compositors can move focus with the pointer; close explicitly.
    pub fn lost_focus(&self) -> bool {
        false
    }
}

/// Other platforms open the browser.
#[cfg(not(any(windows, target_os = "linux")))]
pub struct Popup;

#[cfg(not(any(windows, target_os = "linux")))]
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

#[cfg(all(test, target_os = "linux"))]
mod tests {
    /// Requires X11/Wayland, a D-Bus session and WebKitGTK's sandbox support.
    /// Run alone: cargo test linux_settings_window -- --ignored --test-threads=1
    #[test]
    #[ignore]
    fn linux_settings_window_renders_preview_buttons() {
        use std::{
            sync::{
                atomic::{AtomicBool, Ordering},
                Arc, Mutex,
            },
            time::{Duration, Instant},
        };
        use tao::{
            event_loop::{ControlFlow, EventLoopBuilder},
            platform::{run_return::EventLoopExtRunReturn, unix::EventLoopBuilderExtUnix},
        };
        let config = Arc::new(Mutex::new(crate::config::Config::default()));
        let server = crate::settings::SettingsServer::start(
            config,
            Arc::new(Mutex::new(Vec::new())),
            Arc::new(Mutex::new(None)),
            Arc::new(Mutex::new(None)),
            Arc::new(Mutex::new(Vec::new())),
        );
        let mut event_loop = EventLoopBuilder::new().with_any_thread(true).build();
        let mut popup =
            super::Popup::new(&event_loop, &server.url()).expect("native settings window");
        popup.show(None);
        let passed = Arc::new(AtomicBool::new(false));
        let started = Instant::now();
        let mut next_check = Instant::now();
        event_loop.run_return(|_, _, flow| {
            *flow = ControlFlow::WaitUntil(Instant::now() + Duration::from_millis(50));
            if passed.load(Ordering::Relaxed) || started.elapsed() > Duration::from_secs(20) {
                *flow = ControlFlow::Exit;
            } else if Instant::now() >= next_check {
                next_check = Instant::now() + Duration::from_millis(300);
                let passed = Arc::clone(&passed);
                popup._webview.evaluate_script_with_callback(r#"(() => {
                    if(document.getElementById('status')?.textContent !== 'Running') return false;
                    drawPreview({app_name:'PhotoCraft',document:'Poster',state:'Editing',buttons:[['<Portfolio>','https://example.com/me'],['Unsafe','javascript:alert(1)']]});
                    config.rules=[{app:'PhotoCraft',element:'Crop',activity:'Cropping'}];drawRules();
                    if(document.querySelectorAll('#rules select').length!==1 || document.querySelector('#rules input').value!=='Crop' || !document.getElementById('resources') || !document.getElementById('support-export') || !document.querySelector('.app-idle'))return false;
                    const buttons=document.querySelectorAll('#preview-buttons a');
                    return buttons.length===1 && buttons[0].textContent==='<Portfolio>' && buttons[0].href==='https://example.com/me' && buttons[0].rel==='noopener noreferrer' && document.getElementById('idle-after').value==='300' && document.getElementById('idle-less').getAttribute('aria-label').includes('30 seconds') && document.getElementById('idle-more').getAttribute('aria-label').includes('30 seconds') && getComputedStyle(document.getElementById('idle-after')).backgroundColor==='rgba(0, 0, 0, 0)';
                })()"#, move |value| { if value == "true" { passed.store(true, Ordering::Relaxed); } }).unwrap();
            }
        });
        popup.hide();
        assert!(
            passed.load(Ordering::Relaxed),
            "settings must load, authenticate and render safe link buttons in WebKit"
        );
    }
}
