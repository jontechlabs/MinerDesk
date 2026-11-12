fn main() {
    if let Err(error) = minerdesk_update_helper::run() {
        eprintln!("MinerDesk update: {error}");
        std::process::exit(1);
    }
}
