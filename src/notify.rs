//! Desktop notifications. Only Windows shows them; elsewhere this does nothing.

/// Shows a short toast. Failures are ignored: a missed toast is not an error.
pub fn toast(title: &str, body: &str) {
    #[cfg(windows)]
    {
        let (title, body) = (title.to_string(), body.to_string());
        // Showing a toast can block briefly, so keep it off the main loop.
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
    #[cfg(not(windows))]
    {
        let _ = (title, body);
    }
}
