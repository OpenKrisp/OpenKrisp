use std::error::Error;

use cpal::traits::{DeviceTrait, HostTrait};

fn main() -> Result<(), Box<dyn Error>> {
    let available_hosts = cpal::available_hosts();
    let host = cpal::default_host();

    println!("Available CPAL hosts: {available_hosts:?}");
    println!("Selected default host: {}", host.id());

    let default_input_id = host
        .default_input_device()
        .and_then(|device| device.id().ok());
    let default_output_id = host
        .default_output_device()
        .and_then(|device| device.id().ok());

    let mut device_count = 0_usize;
    for device in host.devices()? {
        device_count += 1;
        let id = device.id()?;
        let description = device.description()?;
        let default_roles = match (
            default_input_id.as_ref() == Some(&id),
            default_output_id.as_ref() == Some(&id),
        ) {
            (true, true) => " [default input, default output]",
            (true, false) => " [default input]",
            (false, true) => " [default output]",
            (false, false) => "",
        };

        println!("\n{} ({id}){default_roles}", description.name());
        println!("  direction: {:?}", description.direction());
        println!("  type: {:?}", description.device_type());
        println!("  interface: {:?}", description.interface_type());
        if let Some(manufacturer) = description.manufacturer() {
            println!("  manufacturer: {manufacturer}");
        }

        match device.default_input_config() {
            Ok(config) => println!("  default input: {config:?}"),
            Err(error) => println!("  default input: unavailable ({})", error.kind()),
        }
        match device.default_output_config() {
            Ok(config) => println!("  default output: {config:?}"),
            Err(error) => println!("  default output: unavailable ({})", error.kind()),
        }
    }

    println!("\nEnumerated {device_count} device(s).");
    Ok(())
}
