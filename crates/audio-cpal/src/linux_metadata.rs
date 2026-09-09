//! PipeWire nodes do not always repeat the parent device's bus properties. Resolve ALSA
//! hardware addresses through sysfs instead of treating a display-name prefix as physical.

use audio_protocol::DeviceKind;

pub(super) fn device_kind(description: &cpal::DeviceDescription) -> DeviceKind {
    let Some(address) = description.address() else {
        return DeviceKind::Unknown;
    };
    match description.driver() {
        Some("api.alsa.pcm.source" | "api.alsa.pcm.sink") => {
            let Some(card) = alsa_card(address) else {
                return DeviceKind::Unknown;
            };
            let subsystem =
                std::fs::canonicalize(format!("/sys/class/sound/card{card}/device/subsystem"));
            if subsystem.as_deref().is_ok_and(|path| {
                matches!(
                    path.to_str(),
                    Some("/sys/bus/pci" | "/sys/bus/usb" | "/sys/bus/firewire")
                )
            }) {
                DeviceKind::Physical
            } else {
                DeviceKind::Unknown
            }
        }
        Some(
            "api.bluez5.sco.source"
            | "api.bluez5.a2dp.source"
            | "api.bluez5.sco.sink"
            | "api.bluez5.a2dp.sink",
        ) if bluetooth_address(address) => DeviceKind::Physical,
        _ => DeviceKind::Unknown,
    }
}

fn alsa_card(address: &str) -> Option<u32> {
    let (kind, rest) = address.split_once(':')?;
    if !matches!(kind, "hw" | "front" | "sysdefault") {
        return None;
    }
    let card = rest.split(',').next()?;
    if card.is_empty() || !card.bytes().all(|c| c.is_ascii_digit()) {
        return None;
    }
    // Never use arbitrary native address text as part of a filesystem path.
    card.parse().ok()
}

fn bluetooth_address(address: &str) -> bool {
    let parts: Vec<_> = address.split(':').collect();
    parts.len() == 6
        && parts
            .iter()
            .all(|part| part.len() == 2 && part.bytes().all(|c| c.is_ascii_hexdigit()))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn hardware_addresses_are_parsed_without_trusting_names_or_paths() {
        assert_eq!(alsa_card("hw:2,0"), Some(2));
        assert_eq!(alsa_card("front:0"), Some(0));
        for address in [
            "default",
            "pulse:0",
            "hw:CARD=Loopback",
            "hw:../../0",
            "hw:",
        ] {
            assert_eq!(alsa_card(address), None);
        }
        assert!(bluetooth_address("AA:bb:00:11:22:33"));
        assert!(!bluetooth_address("My headset"));
        assert!(!bluetooth_address("AA:BB:CC"));
    }
}
