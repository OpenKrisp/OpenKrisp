use std::sync::atomic::{AtomicU8, Ordering};

use serde::Deserialize;
use tauri::{AppHandle, Manager, Runtime, menu::Menu};

use crate::api_error::ApiError;

#[derive(Clone, Copy, Debug, Default, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
#[repr(u8)]
pub(crate) enum AppLanguage {
    #[default]
    En,
    Es,
    #[serde(rename = "pt-BR")]
    PtBr,
    Fr,
    De,
}

impl AppLanguage {
    #[cfg(any(target_os = "macos", test))]
    const ALL: [Self; 5] = [Self::En, Self::Es, Self::PtBr, Self::Fr, Self::De];

    pub(crate) const fn text(self, labels: [&'static str; 5]) -> &'static str {
        labels[self as usize]
    }

    #[cfg(any(target_os = "macos", test))]
    pub(crate) fn uninstaller_alert(self) -> String {
        let title = self.text([
            "Uninstaller unavailable",
            "Desinstalador no disponible",
            "Desinstalador indisponível",
            "Programme de désinstallation indisponible",
            "Deinstallationsprogramm nicht verfügbar",
        ]);
        let message = self.text([
            "Run the latest OpenKrisp installer to restore Uninstall OpenKrisp in Applications, then try again.",
            "Ejecuta el instalador más reciente de OpenKrisp para restaurar Uninstall OpenKrisp en Aplicaciones y vuelve a intentarlo.",
            "Execute o instalador mais recente do OpenKrisp para restaurar Uninstall OpenKrisp em Aplicativos e tente novamente.",
            "Lancez le dernier programme d’installation d’OpenKrisp pour restaurer Uninstall OpenKrisp dans Applications, puis réessayez.",
            "Führe das neueste Installationsprogramm von OpenKrisp aus, um Uninstall OpenKrisp unter Programme wiederherzustellen, und versuche es erneut.",
        ]);
        format!("display alert \"{title}\" message \"{message}\"")
    }
}

pub(crate) struct LanguageMenus<R: Runtime> {
    pub(crate) tray: Menu<R>,
    pub(crate) language: AtomicU8,
}

#[cfg(target_os = "macos")]
impl<R: Runtime> LanguageMenus<R> {
    pub(crate) fn current_language(&self) -> AppLanguage {
        AppLanguage::ALL
            .get(usize::from(self.language.load(Ordering::Relaxed)))
            .copied()
            .unwrap_or_default()
    }
}

#[cfg(any(target_os = "macos", test))]
const UNINSTALL_LABELS: [&str; 5] = [
    "Uninstall OpenKrisp…",
    "Desinstalar OpenKrisp…",
    "Desinstalar OpenKrisp…",
    "Désinstaller OpenKrisp…",
    "OpenKrisp deinstallieren…",
];

#[tauri::command]
pub(crate) fn set_app_language<R: Runtime>(
    app: AppHandle<R>,
    language: AppLanguage,
) -> Result<(), ApiError> {
    let menus = app
        .try_state::<LanguageMenus<R>>()
        .ok_or_else(|| ApiError::internal("Application menus are unavailable."))?;
    menus.language.store(language as u8, Ordering::Relaxed);
    for (id, labels) in [
        (
            "show",
            [
                "Show OpenKrisp",
                "Mostrar OpenKrisp",
                "Mostrar OpenKrisp",
                "Afficher OpenKrisp",
                "OpenKrisp anzeigen",
            ],
        ),
        ("quit", ["Quit", "Salir", "Encerrar", "Quitter", "Beenden"]),
    ] {
        if let Some(item) = menus
            .tray
            .get(id)
            .and_then(|item| item.as_menuitem().cloned())
        {
            item.set_text(language.text(labels))
                .map_err(|error| ApiError::internal(error.to_string()))?;
        }
    }
    #[cfg(target_os = "macos")]
    if let Some(menu) = app.menu() {
        translate_menu(&menu.items().map_err(menu_error)?, language).map_err(menu_error)?;
    }
    Ok(())
}

#[cfg(target_os = "macos")]
fn menu_error(error: tauri::Error) -> ApiError {
    ApiError::internal(error.to_string())
}

#[cfg(target_os = "macos")]
fn translate_menu<R: Runtime>(
    items: &[tauri::menu::MenuItemKind<R>],
    language: AppLanguage,
) -> tauri::Result<()> {
    use tauri::menu::MenuItemKind;
    for item in items {
        match item {
            MenuItemKind::Submenu(submenu) => {
                if let Some(label) = menu_label(&submenu.text()?, language) {
                    submenu.set_text(label)?;
                }
                translate_menu(&submenu.items()?, language)?;
            }
            MenuItemKind::Predefined(item) => {
                if let Some(label) = menu_label(&item.text()?, language) {
                    item.set_text(label)?;
                }
            }
            MenuItemKind::MenuItem(item) if item.id().as_ref() == "uninstall" => {
                item.set_text(language.text(UNINSTALL_LABELS))?;
            }
            _ => {}
        }
    }
    Ok(())
}

#[cfg(any(target_os = "macos", test))]
const MENU_LABELS: &[[&str; 5]] = &[
    ["File", "Archivo", "Arquivo", "Fichier", "Ablage"],
    ["Edit", "Edición", "Editar", "Édition", "Bearbeiten"],
    [
        "View",
        "Visualización",
        "Visualizar",
        "Présentation",
        "Darstellung",
    ],
    ["Window", "Ventana", "Janela", "Fenêtre", "Fenster"],
    ["Help", "Ayuda", "Ajuda", "Aide", "Hilfe"],
    ["About", "Acerca de", "Sobre", "À propos", "Über"],
    [
        "About OpenKrisp",
        "Acerca de OpenKrisp",
        "Sobre o OpenKrisp",
        "À propos d’OpenKrisp",
        "Über OpenKrisp",
    ],
    [
        "Hide OpenKrisp",
        "Ocultar OpenKrisp",
        "Ocultar OpenKrisp",
        "Masquer OpenKrisp",
        "OpenKrisp ausblenden",
    ],
    [
        "Quit OpenKrisp",
        "Salir de OpenKrisp",
        "Encerrar OpenKrisp",
        "Quitter OpenKrisp",
        "OpenKrisp beenden",
    ],
    ["Services", "Servicios", "Serviços", "Services", "Dienste"],
    ["Hide", "Ocultar", "Ocultar", "Masquer", "Ausblenden"],
    [
        "Hide Others",
        "Ocultar otros",
        "Ocultar outros",
        "Masquer les autres",
        "Andere ausblenden",
    ],
    [
        "Show All",
        "Mostrar todo",
        "Mostrar tudo",
        "Tout afficher",
        "Alle einblenden",
    ],
    ["Quit", "Salir", "Encerrar", "Quitter", "Beenden"],
    [
        "Close Window",
        "Cerrar ventana",
        "Fechar janela",
        "Fermer la fenêtre",
        "Fenster schließen",
    ],
    ["Undo", "Deshacer", "Desfazer", "Annuler", "Rückgängig"],
    ["Redo", "Rehacer", "Refazer", "Rétablir", "Wiederholen"],
    ["Cut", "Cortar", "Recortar", "Couper", "Ausschneiden"],
    ["Copy", "Copiar", "Copiar", "Copier", "Kopieren"],
    ["Paste", "Pegar", "Colar", "Coller", "Einsetzen"],
    [
        "Select All",
        "Seleccionar todo",
        "Selecionar tudo",
        "Tout sélectionner",
        "Alles auswählen",
    ],
    [
        "Minimize",
        "Minimizar",
        "Minimizar",
        "Réduire",
        "Minimieren",
    ],
    ["Zoom", "Zoom", "Zoom", "Agrandir", "Zoomen"],
    [
        "Enter Full Screen",
        "Entrar en pantalla completa",
        "Entrar em tela cheia",
        "Activer le mode plein écran",
        "Vollbildmodus aktivieren",
    ],
    [
        "Toggle Full Screen",
        "Alternar pantalla completa",
        "Alternar tela cheia",
        "Basculer le mode plein écran",
        "Vollbildmodus umschalten",
    ],
    [
        "Bring All to Front",
        "Traer todo al frente",
        "Trazer todas para frente",
        "Tout ramener au premier plan",
        "Alle nach vorne",
    ],
];

#[cfg(any(target_os = "macos", test))]
fn menu_label(current: &str, language: AppLanguage) -> Option<&'static str> {
    // Match every supported translation so switching between any pair restores the
    // same native actions and keyboard shortcuts. Mnemonic markers are not semantic.
    let current = current.replace('&', "");
    MENU_LABELS
        .iter()
        .find(|labels| labels.contains(&current.as_str()))
        .map(|labels| language.text(*labels))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn language_commands_only_accept_supported_locales() {
        for (code, language) in [
            ("en", AppLanguage::En),
            ("es", AppLanguage::Es),
            ("pt-BR", AppLanguage::PtBr),
            ("fr", AppLanguage::Fr),
            ("de", AppLanguage::De),
        ] {
            assert_eq!(
                serde_json::from_value::<AppLanguage>(serde_json::json!(code)).unwrap(),
                language
            );
        }
        for unsupported in ["es-AR", "fr-CA", "pt", "pt-PT", "it", "system"] {
            assert!(serde_json::from_value::<AppLanguage>(serde_json::json!(unsupported)).is_err());
        }
    }

    #[test]
    fn native_labels_translate_between_every_pair_of_languages() {
        for labels in MENU_LABELS {
            for source in AppLanguage::ALL {
                for target in AppLanguage::ALL {
                    assert_eq!(
                        menu_label(source.text(*labels), target),
                        Some(target.text(*labels))
                    );
                }
            }
        }
        assert_eq!(menu_label("&Copy", AppLanguage::De), Some("Kopieren"));
        assert_eq!(menu_label("OpenKrisp", AppLanguage::Fr), None);
    }

    #[test]
    fn every_language_has_an_uninstaller_label_and_recovery_alert() {
        for language in AppLanguage::ALL {
            assert!(language.text(UNINSTALL_LABELS).contains("OpenKrisp"));
            let alert = language.uninstaller_alert();
            assert!(alert.starts_with("display alert \""));
            assert!(alert.contains("Uninstall OpenKrisp"));
            assert_eq!(alert.matches('"').count(), 4);
        }
    }
}
