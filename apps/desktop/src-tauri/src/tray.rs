use tauri::{
    App, AppHandle, Manager, Runtime,
    image::Image,
    menu::MenuBuilder,
    tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent},
};

const SHOW_MENU_ID: &str = "show";
const QUIT_MENU_ID: &str = "quit";

pub fn install<R: Runtime>(app: &App<R>) -> tauri::Result<()> {
    #[cfg(target_os = "macos")]
    {
        use tauri::menu::{Menu, MenuItem, PredefinedMenuItem};
        let menu = Menu::default(app.handle())?;
        if let Some(submenu) = menu.items()?.first().and_then(|item| item.as_submenu()) {
            submenu.append(&PredefinedMenuItem::separator(app)?)?;
            submenu.append(&MenuItem::with_id(
                app,
                "uninstall",
                "Uninstall OpenKrisp…",
                true,
                None::<&str>,
            )?)?;
        }
        app.set_menu(menu)?;
        app.on_menu_event(|app, event| {
            if event.id().as_ref() == "uninstall" {
                // The standalone app owns confirmation, authorization and quitting
                // OpenKrisp. Cancellation leaves the main application running.
                let language = app
                    .try_state::<crate::language::LanguageMenus<R>>()
                    .map(|menus| menus.current_language())
                    .unwrap_or_default();
                tauri::async_runtime::spawn_blocking(move || {
                    let result = std::process::Command::new("/usr/bin/open")
                        .arg("/Applications/Uninstall OpenKrisp.app")
                        .status();
                    if !result.is_ok_and(|status| status.success()) {
                        let _ = std::process::Command::new("/usr/bin/osascript")
                            .args(["-e", &language.uninstaller_alert()])
                            .status();
                    }
                });
            }
        });
    }
    let menu = MenuBuilder::new(app)
        .text(SHOW_MENU_ID, "Show OpenKrisp")
        .separator()
        .text(QUIT_MENU_ID, "Quit")
        .build()?;
    app.manage(crate::language::LanguageMenus {
        tray: menu.clone(),
        language: std::sync::atomic::AtomicU8::new(crate::language::AppLanguage::En as u8),
    });
    TrayIconBuilder::with_id("openkrisp")
        .tooltip("OpenKrisp")
        .icon(tray_image())
        .icon_as_template(cfg!(target_os = "macos"))
        .menu(&menu)
        .on_menu_event(|app, event| dispatch_menu(app, event.id().as_ref()))
        .on_tray_icon_event(|tray, event| {
            if matches!(
                event,
                TrayIconEvent::Click {
                    button: MouseButton::Left,
                    button_state: MouseButtonState::Up,
                    ..
                }
            ) {
                show_main_window(tray.app_handle());
            }
        })
        .build(app)?;
    Ok(())
}

pub(crate) fn dispatch_menu<R: Runtime>(app: &AppHandle<R>, id: &str) {
    match id {
        SHOW_MENU_ID => show_main_window(app),
        QUIT_MENU_ID => app.exit(0),
        _ => {}
    }
}

fn show_main_window<R: Runtime>(app: &AppHandle<R>) {
    if let Some(window) = app.get_webview_window("main") {
        let _ = window.show();
        let _ = window.set_focus();
    }
}

fn tray_image() -> Image<'static> {
    // macOS displays this at 18 points. Keep a 2x bitmap for Retina menu bars.
    const SIZE: usize = 36;
    const SAMPLES: usize = 4;
    const SCALE: f32 = 28.0 / SIZE as f32;
    // Match the three rounded, tilted strokes of the desktop wordmark.
    const STROKES: [(f32, f32); 3] = [(7.0, 13.0), (14.0, 25.0), (21.0, 18.0)];
    let (sin, cos) = 18_f32.to_radians().sin_cos();
    // AppKit supplies the color for template images, including selection and
    // appearance changes. Other platforms retain the app's mint accent.
    let color = if cfg!(target_os = "macos") {
        [0, 0, 0]
    } else {
        [0x63, 0xd9, 0xb3]
    };
    let mut rgba = vec![0_u8; SIZE * SIZE * 4];
    for y in 0..SIZE {
        for x in 0..SIZE {
            let mut coverage = 0;
            for sample_y in 0..SAMPLES {
                for sample_x in 0..SAMPLES {
                    let px = (x as f32 + (sample_x as f32 + 0.5) / SAMPLES as f32) * SCALE;
                    let py = (y as f32 + (sample_y as f32 + 0.5) / SAMPLES as f32) * SCALE;
                    if STROKES.iter().any(|&(center_x, height)| {
                        let dx = px - center_x;
                        let dy = py - 14.0;
                        let local_x = cos * dx + sin * dy;
                        let local_y = -sin * dx + cos * dy;
                        let cap = (local_y.abs() - (height / 2.0 - 2.0)).max(0.0);
                        local_x * local_x + cap * cap <= 4.0
                    }) {
                        coverage += 1;
                    }
                }
            }
            let offset = (y * SIZE + x) * 4;
            rgba[offset..offset + 3].copy_from_slice(&color);
            rgba[offset + 3] = (coverage * 255 / (SAMPLES * SAMPLES)) as u8;
        }
    }
    Image::new_owned(rgba, SIZE as u32, SIZE as u32)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn generated_tray_icon_has_bounded_rgba_dimensions() {
        let icon = tray_image();
        assert_eq!(icon.width(), 36);
        assert_eq!(icon.height(), 36);
        assert_eq!(icon.rgba().len(), 36 * 36 * 4);
        assert!(
            icon.rgba()
                .as_chunks::<4>()
                .0
                .iter()
                .any(|pixel| pixel[3] == 255)
        );
    }
}
