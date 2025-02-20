use std::{env, path::PathBuf};
use dotenvy::dotenv;

const HELP_STRING: &str = "\
rawdb - A simple image archiver
usage: rawdb [-options] [source_dir]
    [--target <target_dir>]
    [--db <database_file>]
";

pub struct AppArgs {
    pub source_dir: PathBuf,
    pub target_dir: PathBuf,
    pub database_path: PathBuf,
    pub clean: bool,
}

pub fn parse_args() -> anyhow::Result<AppArgs> {
    dotenv()?;
    let mut pargs = pico_args::Arguments::from_env();

    if pargs.contains(["-h", "--help"]) {
        print!("{}", HELP_STRING);
        std::process::exit(0);
    }

    let source_dir = pargs.free_from_str()?;

    let target_dir = pargs
        .opt_value_from_str("--target")?
        .or_else(|| env::var_os("RAWDB_TARGET").map(PathBuf::from))
        .ok_or_else(|| anyhow::anyhow!("--target or RAWDB_TARGET must be set"))?;

    let database_path = pargs
        .opt_value_from_str("--db")?
        .or_else(|| env::var_os("RAWDB_DB").map(PathBuf::from))
        .ok_or_else(|| anyhow::anyhow!("--db or RAWDB_DB must be set"))?;

    let clean = pargs.contains(["-c", "--clean"]);

    let remaining = pargs.finish();
    if !remaining.is_empty() {
        eprintln!("Unrecognized arguments: {:?}", remaining);
    }

    Ok(AppArgs { source_dir, target_dir, database_path, clean })
}

