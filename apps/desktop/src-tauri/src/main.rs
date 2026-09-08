fn main() {
    if let Err(error) = openkrisp_desktop_lib::run() {
        eprintln!("OpenKrisp failed to start: {error}");
        std::process::exit(1);
    }
}
