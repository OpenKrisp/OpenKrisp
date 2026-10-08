//! Release gate: prove that final update bytes match the public key embedded in the app.
use base64::{Engine, engine::general_purpose::STANDARD};
use minisign_verify::{PublicKey, Signature};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let arguments: Vec<_> = std::env::args_os().skip(1).collect();
    if arguments.len() != 2 {
        return Err("usage: verify_updater ARTIFACT SIGNATURE".into());
    }
    let key = std::env::var("OPENKRISP_UPDATER_PUBLIC_KEY")?;
    let key = String::from_utf8(STANDARD.decode(key.trim())?)?;
    let signature = std::fs::read_to_string(&arguments[1])?;
    let signature = String::from_utf8(STANDARD.decode(signature.trim())?)?;
    PublicKey::decode(&key)?.verify(
        &std::fs::read(&arguments[0])?,
        &Signature::decode(&signature)?,
        true,
    )?;
    println!("Verified updater artifact signature.");
    Ok(())
}
