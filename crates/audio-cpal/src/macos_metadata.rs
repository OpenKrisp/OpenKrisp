//! Read-only CoreAudio metadata missing from CPAL 0.18.2. Never called by audio callbacks.

use std::ptr::{NonNull, null};

use audio_protocol::DeviceKind;
use objc2_core_audio::{
    AudioObjectGetPropertyData, AudioObjectPropertyAddress, kAudioDevicePropertyTransportType,
    kAudioHardwarePropertyTranslateUIDToDevice, kAudioObjectPropertyElementMain,
    kAudioObjectPropertyScopeGlobal, kAudioObjectSystemObject,
};
use objc2_core_foundation::CFString;

pub(super) fn device_kind(uid: &str) -> DeviceKind {
    let uid = CFString::from_str(uid);
    let uid_pointer: *const CFString = &*uid;
    let address = AudioObjectPropertyAddress {
        mSelector: kAudioHardwarePropertyTranslateUIDToDevice,
        mScope: kAudioObjectPropertyScopeGlobal,
        mElement: kAudioObjectPropertyElementMain,
    };
    let mut device_id = 0_u32;
    let mut size = size_of::<u32>() as u32;
    // SAFETY: The qualifier is a live CFString pointer, and the output is an aligned,
    // writable AudioObjectID (u32). CoreAudio borrows both for this synchronous query.
    let status = unsafe {
        AudioObjectGetPropertyData(
            kAudioObjectSystemObject as u32,
            NonNull::from(&address),
            size_of::<*const CFString>() as u32,
            std::ptr::from_ref(&uid_pointer).cast(),
            NonNull::from(&mut size),
            NonNull::from(&mut device_id).cast(),
        )
    };
    if status != 0 || size != size_of::<u32>() as u32 || device_id == 0 {
        return DeviceKind::Unknown;
    }
    let address = AudioObjectPropertyAddress {
        mSelector: kAudioDevicePropertyTransportType,
        ..address
    };
    let mut transport = 0_u32;
    // SAFETY: This property returns one UInt32; size and output point to valid writable
    // storage. There is no qualifier. Failed/disconnected queries are classified Unknown.
    let status = unsafe {
        AudioObjectGetPropertyData(
            device_id,
            NonNull::from(&address),
            0,
            null(),
            NonNull::from(&mut size),
            NonNull::from(&mut transport).cast(),
        )
    };
    if status != 0 || size != size_of::<u32>() as u32 {
        return DeviceKind::Unknown;
    }
    classify_transport(transport)
}

fn classify_transport(transport: u32) -> DeviceKind {
    match &transport.to_be_bytes() {
        b"virt" => DeviceKind::Virtual,
        b"grup" | b"fgrp" => DeviceKind::Aggregate,
        b"bltn" | b"usb " | b"pci " | b"1394" | b"blue" | b"blea" | b"hdmi" | b"dprt" | b"thun"
        | b"ccwd" | b"ccwl" => DeviceKind::Physical,
        _ => DeviceKind::Unknown,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn transport_classification_fails_closed() {
        for transport in [*b"bltn", *b"usb ", *b"blue"] {
            assert_eq!(
                classify_transport(u32::from_be_bytes(transport)),
                DeviceKind::Physical
            );
        }
        assert_eq!(
            classify_transport(u32::from_be_bytes(*b"virt")),
            DeviceKind::Virtual
        );
        assert_eq!(
            classify_transport(u32::from_be_bytes(*b"grup")),
            DeviceKind::Aggregate
        );
        assert_eq!(classify_transport(0), DeviceKind::Unknown);
        assert_eq!(
            device_kind("openkrisp-nonexistent-test-device"),
            DeviceKind::Unknown
        );
    }
}
