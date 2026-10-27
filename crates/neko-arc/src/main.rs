mod archive;
mod convert;
mod decode;
mod dictionary;
mod dump;
mod fix;
mod hash;
mod io;

use std::path::{Path, PathBuf};
use std::process::Command as ProcessCommand;

use clap::{Args, CommandFactory, Parser, Subcommand};
use colored::Colorize;
use tracing::{Level, error};
use tracing_subscriber::fmt;

use crate::decode::DecodeRequest;
use crate::fix::FixKinds;
use crate::io::{DEFAULT_LEVEL, beside_binary, file_name_of, get_local_dir, has_extension, walk_files};

#[derive(Parser)]
#[command(name = "neko-arc", version, about = "Battle Cats Switch Archive Toolkit", long_about = None)]
struct Cli {
    #[arg(short, long, global = true, help = "Enable verbose debug logging")]
    verbose: bool,
    #[arg(short = 't', long, global = true, help = "Enable maximum trace-level logging")]
    trace: bool,
    #[arg(short, long, global = true, help = "Output logs in structured JSON format")]
    json: bool,
    #[command(subcommand)]
    command: Option<Commands>,
}

#[derive(Args)]
struct TargetArgs {
    #[arg(
        value_name = "FILE | DIR",
        help = "Targets resolved automatically when no flags are given"
    )]
    input: Vec<String>,
    #[arg(short, long, value_name = "FILE", help = "Target a named file, repeatable")]
    file: Vec<String>,
    #[arg(
        short,
        long,
        num_args = 1..=2,
        value_names = ["DIR", "LEVEL"],
        help = "Target every file in a directory, walking LEVEL deep (default 10)"
    )]
    dir: Option<Vec<String>>,
}

#[derive(Subcommand)]
enum Commands {
    #[command(about = "Dump every RomFS file, FCRA archives included, straight out of an NSP")]
    Dump {
        #[command(flatten)]
        target: TargetArgs,
        #[arg(
            short,
            long,
            value_name = "FILE",
            help = "Use this prod.keys instead of the one beside the binary"
        )]
        keys: Option<String>,
        #[arg(short, long, value_name = "DIR", help = "Override the default output directory")]
        output: Option<String>,
    },
    #[command(about = "Extract FCRA archives into folders of hashed files")]
    Decrypt {
        #[command(flatten)]
        target: TargetArgs,
        #[arg(short, long, value_name = "DIR", help = "Override the default output directory")]
        output: Option<String>,
    },
    #[command(about = "Resolve original asset names into their archive hashes")]
    Decode {
        #[command(flatten)]
        target: TargetArgs,
        #[arg(
            short = 'c',
            long,
            value_name = "FILE",
            help = "Use this {hash},{name} dictionary, renaming falls back to the one beside the binary"
        )]
        dictionary: Option<String>,
        #[arg(
            short,
            long,
            help = "Print every name alongside its hash, or every rename as it happens"
        )]
        print: bool,
        #[arg(
            short,
            long,
            help = "Rename the targeted hashed files from the dictionary, the working directory when none are given"
        )]
        rename: bool,
    },
    #[command(about = "Write a {hash},{name} dictionary for every file in a directory")]
    Dictionary {
        #[arg(value_name = "DIR")]
        input: Option<String>,
        #[arg(value_name = "LEVEL", default_value_t = DEFAULT_LEVEL)]
        level: usize,
        #[arg(short, long, value_name = "PATH", help = "Override the default dictionary path")]
        output: Option<String>,
    },
    #[command(about = "Convert Nintendo containers into Battle Cats formats")]
    Convert {
        #[command(flatten)]
        target: TargetArgs,
        #[arg(
            short,
            long,
            value_name = "DIR",
            help = "Dump converted files here instead of replacing the originals"
        )]
        output: Option<String>,
    },
    #[command(about = "Repair images and tables at the cost of exact fidelity")]
    Fix {
        #[command(flatten)]
        target: TargetArgs,
        #[arg(
            short = 'p',
            long,
            help = "Un-premultiply .png images once, the default when no kind is chosen"
        )]
        png: bool,
        #[arg(short = 'b', long, help = "Decode .bntx containers straight into corrected images")]
        bntx: bool,
        #[arg(short = 'c', long, help = "Write .csb tables out with unparsable fields flattened")]
        csb: bool,
        #[arg(short = 'a', long, help = "Repair every supported kind at once")]
        all: bool,
    },
}

fn main() {
    #[cfg(windows)]
    let _ = colored::control::set_virtual_terminal(true);

    let cli = Cli::parse();
    let show_ui = !cli.json && !cli.verbose && !cli.trace;

    if cli.json {
        colored::control::set_override(false);
        let max_level = if cli.trace {
            Level::TRACE
        } else if cli.verbose {
            Level::DEBUG
        } else {
            Level::INFO
        };
        fmt()
            .json()
            .with_file(true)
            .with_line_number(true)
            .with_max_level(max_level)
            .init();
    } else if cli.trace {
        fmt()
            .with_file(true)
            .with_line_number(true)
            .with_max_level(Level::TRACE)
            .init();
    } else if cli.verbose {
        fmt()
            .with_file(true)
            .with_line_number(true)
            .with_max_level(Level::DEBUG)
            .init();
    }

    match cli.command {
        Some(Commands::Dump { target, keys, output }) => {
            if target.is_empty() {
                print_command_help("dump");
            }
            let targets = resolve_paths(&target, Some("nsp"), show_ui);
            dump::execute(&targets, keys.as_deref(), output.as_deref(), show_ui);
        }
        Some(Commands::Decrypt { target, output }) => {
            if target.is_empty() {
                print_command_help("decrypt");
            }
            let targets = resolve_paths(&target, Some("arc"), show_ui);
            archive::execute(&targets, output.as_deref(), show_ui);
        }
        Some(Commands::Decode {
            target,
            dictionary,
            print,
            rename,
        }) => {
            if target.is_empty() && dictionary.is_none() && !print && !rename {
                print_command_help("decode");
            }
            handle_decode_command(&target, dictionary.as_deref(), print, rename, show_ui);
        }
        Some(Commands::Dictionary { input, level, output }) => {
            let Some(source_dir) = input else {
                print_command_help("dictionary");
            };
            dictionary::execute(&source_dir, level, output.as_deref(), show_ui);
        }
        Some(Commands::Convert { target, output }) => {
            if target.is_empty() && output.is_none() {
                print_command_help("convert");
            }
            let targets = resolve_paths(&target, None, show_ui);
            convert::execute(&targets, output.as_deref(), show_ui);
        }
        Some(Commands::Fix {
            target,
            png,
            bntx,
            csb,
            all,
        }) => {
            if target.is_empty() && !(png || bntx || csb || all) {
                print_command_help("fix");
            }
            let targets = resolve_paths(&target, None, show_ui);
            let kinds = FixKinds {
                png: all || png || !(bntx || csb),
                bntx: all || bntx,
                csb: all || csb,
            };
            fix::execute(&targets, &kinds, show_ui);
        }
        None => handle_fallback_shell(),
    }
}

fn print_command_help(name: &str) -> ! {
    let mut root = Cli::command();
    root.build();

    if let Some(command) = root.find_subcommand_mut(name) {
        let _ = command.print_help();
    } else {
        let _ = root.print_help();
    }

    println!();
    std::process::exit(0);
}

fn handle_decode_command(target: &TargetArgs, dictionary: Option<&str>, print: bool, rename: bool, show_ui: bool) {
    let dictionary = dictionary.map(PathBuf::from);

    if rename {
        let Some(dictionary) =
            dictionary.or_else(|| beside_binary(dictionary::DEFAULT_FILE_NAME).filter(|path| path.is_file()))
        else {
            abort(
                "Renaming needs a dictionary, pass --dictionary or keep dictionary.csv beside the binary",
                show_ui,
            );
        };

        let targets = if target.is_empty() {
            walk_files(&get_local_dir(), DEFAULT_LEVEL)
        } else {
            resolve_paths(target, None, show_ui)
        };

        decode::rename(&targets, &dictionary, print, show_ui);
        return;
    }

    let mut directories: Vec<(PathBuf, usize)> = target.directory(show_ui).into_iter().collect();
    let mut names: Vec<String> = target.file.iter().map(|file| asset_name(file)).collect();

    for input in &target.input {
        if Path::new(input).is_dir() {
            directories.push((PathBuf::from(input), DEFAULT_LEVEL));
        } else {
            names.push(asset_name(input));
        }
    }

    if names.is_empty() && directories.is_empty() && dictionary.is_none() {
        abort(
            "No target given, expected a name, --file, --dir, or --dictionary",
            show_ui,
        );
    }

    let request = DecodeRequest {
        names: &names,
        directories: &directories,
        dictionary: dictionary.as_deref(),
    };

    decode::execute(&request, show_ui);
}

fn asset_name(input: &str) -> String {
    let path = Path::new(input);

    if path.is_file() {
        return file_name_of(path).unwrap_or(input).to_string();
    }

    input.to_string()
}

fn resolve_paths(target: &TargetArgs, walked: Option<&str>, show_ui: bool) -> Vec<PathBuf> {
    let mut paths: Vec<PathBuf> = target.file.iter().map(PathBuf::from).collect();

    if let Some((directory, level)) = target.directory(show_ui) {
        paths.extend(walk_only(&directory, level, walked));
    }

    for input in &target.input {
        let path = Path::new(input);

        if path.is_dir() {
            paths.extend(walk_only(path, DEFAULT_LEVEL, walked));
        } else {
            paths.push(path.to_path_buf());
        }
    }

    if paths.is_empty() {
        abort("No target given, expected a path, --file, or --dir", show_ui);
    }

    paths.sort();
    paths.dedup();
    paths
}

fn walk_only(directory: &Path, level: usize, extension: Option<&str>) -> Vec<PathBuf> {
    let found = walk_files(directory, level);

    let Some(wanted) = extension else {
        return found;
    };

    found.into_iter().filter(|path| has_extension(path, wanted)).collect()
}

fn abort(message: &str, show_ui: bool) -> ! {
    if show_ui {
        println!("\n  {} {}\n", "✗".red(), message);
    }
    error!("{}", message);
    std::process::exit(1);
}

fn handle_fallback_shell() {
    let mut command_instance = Cli::command();
    let _ = command_instance.print_help();

    if cfg!(target_os = "windows") {
        let _ = ProcessCommand::new("cmd.exe").status();
        return;
    }

    let fallback_shell = std::env::var("SHELL").unwrap_or_else(|_| String::from("sh"));
    let _ = ProcessCommand::new(fallback_shell).status();
}

impl TargetArgs {
    fn is_empty(&self) -> bool {
        self.input.is_empty() && self.file.is_empty() && self.dir.is_none()
    }

    fn directory(&self, show_ui: bool) -> Option<(PathBuf, usize)> {
        let values = self.dir.as_ref()?;
        let path = values.first().map_or_else(get_local_dir, PathBuf::from);

        let Some(raw_level) = values.get(1) else {
            return Some((path, DEFAULT_LEVEL));
        };

        let Ok(level) = raw_level.parse::<usize>() else {
            abort(&format!("Invalid walk level: '{raw_level}'"), show_ui);
        };

        Some((path, level))
    }
}
