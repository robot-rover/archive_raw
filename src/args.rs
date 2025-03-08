use anyhow::bail;
use std::{env, ffi::OsStr, path::PathBuf};

const HELP_STRING: &str = "\
rawdb - A simple image archiver
usage: rawdb [-options] [source_dir]
    [--target <target_dir>] # The directory place archived images
    [--db <database_file>]  # The location to store the image database
    [-c | --clean]          # Clear the image database
    [-d | --dry-run]        # Index but don't archive
    [-l | --leave]          # Do not remove temp tables
";

pub struct AppArgs {
    pub source_dir: Option<PathBuf>,
    pub target_dir: PathBuf,
    pub database_path: PathBuf,
    pub clean: bool,
    pub dry: bool,
    pub leave: bool,
}

fn parse_path(os_str: &OsStr) -> Result<PathBuf, &'static str> {
    Ok(PathBuf::from(os_str))
}

pub fn parse_args() -> anyhow::Result<AppArgs> {
    let mut pargs = pico_args::Arguments::from_env();

    if pargs.contains(["-h", "--help"]) {
        print!("{}", HELP_STRING);
        std::process::exit(0);
    }

    let target_dir = pargs
        .opt_value_from_os_str("--target", parse_path)
        .unwrap()
        .or_else(|| env::var_os("RAWDB_TARGET").map(PathBuf::from))
        .ok_or_else(|| anyhow::anyhow!("--target or RAWDB_TARGET must be set"))?;

    let database_path = pargs
        .opt_value_from_os_str("--db", parse_path)
        .unwrap()
        .or_else(|| env::var_os("RAWDB_DB").map(PathBuf::from))
        .ok_or_else(|| anyhow::anyhow!("--db or RAWDB_DB must be set"))?;

    let clean = pargs.contains(["-c", "--clean"]);
    let dry = pargs.contains(["-d", "--dry-run"]);
    let leave = pargs.contains(["-l", "--leave"]);

    let source_dir = pargs.opt_free_from_os_str(parse_path).unwrap();

    let remaining = pargs.finish();
    if !remaining.is_empty() {
        bail!("Unrecognized arguments: {:?}", remaining);
    }

    Ok(AppArgs {
        source_dir,
        target_dir,
        database_path,
        clean,
        dry,
        leave,
    })
}
