use std::error::Error;

use audio_cpal::CpalHost;

fn main() -> Result<(), Box<dyn Error>> {
    let inventory = CpalHost::new_default().inventory()?;
    println!("Selected default host: {}", inventory.host);

    let device_count = inventory.devices.len();
    for device in inventory.devices {
        let default_roles = match (device.is_default_input, device.is_default_output) {
            (true, true) => " [default input, default output]",
            (true, false) => " [default input]",
            (false, true) => " [default output]",
            (false, false) => "",
        };

        println!("\n{} ({}){default_roles}", device.name, device.id);
        println!("  kind: {:?}", device.kind);
        if let Some(driver) = device.driver {
            println!("  driver: {driver}");
        }
        println!(
            "  roles: input={}, output={}",
            device.supports_input, device.supports_output
        );
        if let Some(manufacturer) = device.manufacturer {
            println!("  manufacturer: {manufacturer}");
        }
        if let Some(format) = device.default_input_format {
            println!("  default input: {format:?}");
        }
        if let Some(format) = device.default_output_format {
            println!("  default output: {format:?}");
        }
    }

    println!("\nEnumerated {device_count} device(s).");
    Ok(())
}
