//! `tasq-relay`: the sync server. It stores each account's sealed changes
//! and hands them out; it can't read them (see `tasq::cloud::relay`).
//!
//!     tasq-relay [--addr 0.0.0.0:8080] [--db /data/relay.db]
//!
//! Put it behind HTTPS (a reverse proxy: Traefik, Caddy, nginx).

fn main() -> anyhow::Result<()> {
    let mut addr = std::env::var("TASQ_RELAY_ADDR").unwrap_or_else(|_| "0.0.0.0:8080".into());
    let mut db = std::env::var("TASQ_RELAY_DB").unwrap_or_else(|_| "relay.db".into());
    let mut args = std::env::args().skip(1);
    while let Some(a) = args.next() {
        match a.as_str() {
            "--addr" => addr = args.next().unwrap_or(addr),
            "--db" => db = args.next().unwrap_or(db),
            "-h" | "--help" => {
                println!("usage: tasq-relay [--addr HOST:PORT] [--db PATH]");
                return Ok(());
            }
            other => anyhow::bail!("unknown argument: {other}"),
        }
    }
    let path = std::path::PathBuf::from(&db);
    if let Some(dir) = path.parent().filter(|d| !d.as_os_str().is_empty()) {
        std::fs::create_dir_all(dir)?;
    }
    let relay = tasq::cloud::relay::Relay::open(&path)?;
    eprintln!("tasq-relay listening on {addr}, data in {db}");
    tasq::cloud::relay::serve(&addr, relay)
}
