//! Desktop notifications (Windows toast, macOS notification, Linux D-Bus).

/// Shows a short notification. Failures are ignored: a missed one is not an
/// error (for example a bare Linux window manager has no notification daemon).
pub fn toast(title: &str, body: &str) {
    let (title, body) = (title.to_string(), body.to_string());
    // Showing a notification can block briefly, so keep it off the main loop.
    let _ = std::thread::Builder::new()
        .name("craftpulse-toast".to_string())
        .spawn(move || {
            let _ = notify_rust::Notification::new()
                .appname("CraftPulse")
                .summary(&title)
                .body(&body)
                .show();
        });
}
