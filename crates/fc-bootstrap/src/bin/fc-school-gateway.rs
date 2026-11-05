//! School-local HTTPS/certificate foundation. Built only with school-gateway.
use fc_bootstrap::school_gateway::{self, Result};
use std::path::Path;
fn run() -> Result<()> {
    let args = std::env::args().skip(1).collect::<Vec<_>>();
    match args.first().map(String::as_str) {
        Some("init") if args.len() == 6 => {
            let password = school_gateway::read_password()?;
            let fingerprint = school_gateway::initialize(Path::new(&args[1]), Path::new(&args[2]), &args[3], args[4].parse()?, args[5].parse()?, &password)?;
            println!("School created. Root SHA-256: {fingerprint}. Move encrypted recovery directory offline before service deployment.");
        }
        Some("renew-candidate") if args.len() == 3 => {
            let password = school_gateway::read_password()?;
            school_gateway::renew_candidate(Path::new(&args[1]), Path::new(&args[2]), &password)?;
            println!("Fresh leaf candidate validated. Original state unchanged; activation requires service stop and explicit state-directory promotion.");
        }
        Some("serve") if args.len() == 2 => {
            let password = school_gateway::read_password()?;
            school_gateway::server::serve(Path::new(&args[1]), &password)?;
        }
        _ => return Err("usage: fc-school-gateway init <new-state-dir> <new-offline-recovery-dir> <school-name> <private-ip> <port> | serve <state-dir> | renew-candidate <state-dir> <new-candidate-dir>; provide one secret password line on stdin, never argv".into()),
    }
    Ok(())
}
fn main() {
    if let Err(error) = run() {
        eprintln!("School gateway: {error}");
        std::process::exit(1);
    }
}
