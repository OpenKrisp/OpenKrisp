use tauri::{
    App, AppHandle, Manager, Runtime,
    image::Image,
    menu::MenuBuilder,
    tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent},
};

const SHOW_MENU_ID: &str = "show";
const QUIT_MENU_ID: &str = "quit";

pub fn install<R: Runtime>(app: &App<R>) -> tauri::Result<()> {
    let menu = MenuBuilder::new(app)
        .text(SHOW_MENU_ID, "Show OpenKrisp")
        .separator()
        .text(QUIT_MENU_ID, "Quit")
        .build()?;
    TrayIconBuilder::with_id("openkrisp")
        .tooltip("OpenKrisp")
        .icon(tray_image())
        .menu(&menu)
        .on_menu_event(|app, event| match event.id().as_ref() {
            SHOW_MENU_ID => show_main_window(app),
            QUIT_MENU_ID => app.exit(0),
            _ => {}
        })
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

fn show_main_window<R: Runtime>(app: &AppHandle<R>) {
    if let Some(window) = app.get_webview_window("main") {
        let _ = window.show();
        let _ = window.set_focus();
    }
}

fn tray_image() -> Image<'static> {
    const SIZE: usize = 16;
    let mut rgba = vec![0_u8; SIZE * SIZE * 4];
    for y in 2..14 {
        for x in 3..13 {
            let stem = (3..=5).contains(&x);
            let upper = x >= 6 && x + y == 14;
            let lower = x >= 6 && x == y - 1;
            if stem || upper || lower {
                let offset = (y * SIZE + x) * 4;
                rgba[offset] = 28;
                rgba[offset + 1] = 188;
                rgba[offset + 2] = 142;
                rgba[offset + 3] = 255;
            }
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
        assert_eq!(icon.width(), 16);
        assert_eq!(icon.height(), 16);
        assert_eq!(icon.rgba().len(), 16 * 16 * 4);
        assert!(
            icon.rgba()
                .as_chunks::<4>()
                .0
                .iter()
                .any(|pixel| pixel[3] == 255)
        );
    }
}
