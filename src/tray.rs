use std::{
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
    thread,
    time::Duration,
};

pub fn start(settings_url: String) -> Arc<AtomicBool> {
    let quit = Arc::new(AtomicBool::new(false));
    let thread_quit = Arc::clone(&quit);
    thread::Builder::new()
        .name("craftpulse-tray".to_string())
        .spawn(move || run_tray(settings_url, thread_quit))
        .expect("could not create tray thread");
    quit
}

fn run_tray(settings_url: String, quit: Arc<AtomicBool>) {
    use tao::{
        event::{Event, WindowEvent},
        event_loop::{ControlFlow, EventLoopBuilder},
    };
    use tray_icon::{
        menu::{Menu, MenuEvent, MenuItem, PredefinedMenuItem},
        Icon, TrayIconBuilder, TrayIconEvent,
    };

    // On Windows, tao refuses to create an event loop off the main thread
    // unless `with_any_thread(true)` is set. The tray runs on its own thread.
    #[allow(unused_mut)]
    let mut builder = EventLoopBuilder::new();
    #[cfg(windows)]
    {
        use tao::platform::windows::EventLoopBuilderExtWindows;
        builder.with_any_thread(true);
    }
    let event_loop = builder.build();

    let menu = Menu::new();
    let settings = MenuItem::new("Open CraftPulse settings", true, None);
    let quit_item = MenuItem::new("Quit", true, None);
    let _ = menu.append_items(&[&settings, &PredefinedMenuItem::separator(), &quit_item]);
    let settings_id = settings.id().clone();
    let quit_id = quit_item.id().clone();
    let icon = Icon::from_rgba(icon_pixels(), 32, 32).expect("invalid tray icon image");
    let _tray = TrayIconBuilder::new()
        .with_menu(Box::new(menu))
        .with_tooltip("CraftPulse — ArtCraft presence")
        .with_icon(icon)
        .build()
        .expect("could not create tray icon");
    let menu_events = MenuEvent::receiver();
    let tray_events = TrayIconEvent::receiver();
    let url = settings_url.clone();

    event_loop.run(move |event, _, control_flow| {
        *control_flow =
            ControlFlow::WaitUntil(std::time::Instant::now() + Duration::from_millis(250));
        if quit.load(Ordering::Relaxed) {
            *control_flow = ControlFlow::Exit;
            return;
        }
        if let Event::WindowEvent {
            event: WindowEvent::CloseRequested,
            ..
        } = event
        {
            *control_flow = ControlFlow::Exit;
        }
        while let Ok(event) = menu_events.try_recv() {
            if event.id == settings_id {
                let _ = open::that(&url);
            }
            if event.id == quit_id {
                quit.store(true, Ordering::Relaxed);
                *control_flow = ControlFlow::Exit;
            }
        }
        while let Ok(event) = tray_events.try_recv() {
            if matches!(
                event,
                TrayIconEvent::Click {
                    button: tray_icon::MouseButton::Left,
                    button_state: tray_icon::MouseButtonState::Up,
                    ..
                } | TrayIconEvent::DoubleClick {
                    button: tray_icon::MouseButton::Left,
                    ..
                }
            ) {
                let _ = open::that(&url);
            }
        }
    });
}

fn icon_pixels() -> Vec<u8> {
    let mut pixels = Vec::with_capacity(32 * 32 * 4);
    for y in 0..32 {
        for x in 0..32 {
            let dx: i32 = x - 16;
            let dy: i32 = y - 16;
            let in_circle = dx * dx + dy * dy <= 225;
            let stroke = in_circle && (dx.abs() <= 2 || dy.abs() <= 2);
            let (r, g, b, a) = if !in_circle {
                (0, 0, 0, 0)
            } else if stroke {
                (245, 125, 45, 255)
            } else {
                (28, 31, 39, 255)
            };
            pixels.extend_from_slice(&[r, g, b, a]);
        }
    }
    pixels
}
