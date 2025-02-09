use anyhow::Error;
use rusync::console_info::ConsoleProgressInfo;
use rusync::sync::SyncOptions;
use rusync::Syncer;
use std::path::PathBuf;
use std::process;
use structopt::StructOpt;

#[derive(Debug, StructOpt)]
#[structopt(name = "rusync")]
struct Opt {
    #[structopt(
        long = "no-perms",
        help = "Do not preserve permissions (no-op on Windows)"
    )]
    no_preserve_permissions: bool,

    #[structopt(long = "err-list", help = "Write errors to the given file")]
    error_list_path: Option<PathBuf>,

    #[structopt(parse(from_os_str))]
    source: PathBuf,

    #[structopt(parse(from_os_str))]
    destination: PathBuf,
}

fn main() -> Result<(), Error> {
    let opt = Opt::from_args();
    let source = &opt.source;
    if !source.is_dir() {
        eprintln!("{} is not a directory", source.to_string_lossy());
        process::exit(1);
    }
    let destination = &opt.destination;

    let console_info = match opt.error_list_path {
        Some(err_file) => ConsoleProgressInfo::with_error_list_path(&err_file)?,
        None => ConsoleProgressInfo::new(),
    };
    let options = SyncOptions {
        preserve_permissions: !opt.no_preserve_permissions,
    };
    let syncer = Syncer::new(source, destination, options, Box::new(console_info));
    let stats = syncer.sync();
    match stats {
        Err(err) => {
            eprintln!("{}", err);
            process::exit(1);
        }
        Ok(stats) if stats.errors > 0 => {
            process::exit(1);
        }
        _ => {
            process::exit(0);
        }
    }
}
