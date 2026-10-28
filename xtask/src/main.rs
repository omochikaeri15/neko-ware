use std::env;
use std::fs;
use std::process::{Command, exit};

const RELEASE_BRANCH: &str = "main";
const MANIFEST: &str = "Cargo.toml";
const LOCKFILE: &str = "Cargo.lock";
const TAG_PREFIX: &str = "v";

fn run_git(args: &[&str]) {
    let status = Command::new("git")
        .args(args)
        .status()
        .expect("Failed to execute git command");

    if !status.success() {
        eprintln!("Error: 'git {}' failed. Aborting.", args.join(" "));
        exit(1);
    }
}

fn run_cargo(args: &[&str]) {
    let status = Command::new("cargo")
        .args(args)
        .status()
        .expect("Failed to execute cargo command");

    if !status.success() {
        eprintln!("Error: 'cargo {}' failed. Aborting.", args.join(" "));
        exit(1);
    }
}

fn git_succeeds(args: &[&str]) -> bool {
    Command::new("git")
        .args(args)
        .output()
        .map(|output| output.status.success())
        .unwrap_or(false)
}

fn parse_version(text: &str) -> Option<(u64, u64, u64)> {
    let mut parts = text.trim().split('.').map(|part| part.parse::<u64>().ok());
    let version = (parts.next()??, parts.next()??, parts.next()??);

    parts.next().is_none().then_some(version)
}

fn manifest_version() -> String {
    let manifest = fs::read_to_string(MANIFEST).expect("Failed to read the workspace manifest");

    manifest
        .lines()
        .map(str::trim)
        .find_map(|line| line.strip_prefix("version")?.trim_start().strip_prefix('=')?.trim().strip_prefix('"')?.strip_suffix('"'))
        .expect("The workspace manifest has no version line")
        .to_string()
}

fn this_month() -> (u64, u64) {
    let output = Command::new("date").arg("+%y %-m").output().expect("Failed to execute date");
    let text = String::from_utf8_lossy(&output.stdout);
    let mut parts = text.split_whitespace().map(|part| part.parse::<u64>().ok());

    let (Some(Some(year)), Some(Some(month))) = (parts.next(), parts.next()) else {
        eprintln!("Error: could not read the current year and month from 'date'.");
        exit(1);
    };

    (year, month)
}

fn next_version(current: Option<(u64, u64, u64)>) -> String {
    let (year, month) = this_month();
    let count = current
        .filter(|&(y, m, _)| (y, m) == (year, month))
        .map_or(0, |(_, _, n)| n + 1);

    format!("{year}.{month}.{count}")
}

fn stamp_version(old: &str, new: &str) {
    let manifest = fs::read_to_string(MANIFEST).expect("Failed to read the workspace manifest");
    let stamped = manifest.replacen(&format!("version = \"{old}\""), &format!("version = \"{new}\""), 1);

    fs::write(MANIFEST, stamped).expect("Failed to write the workspace manifest");
}

fn ship() {
    println!("Starting Neko-Ware Release...");

    println!("> Checking out {RELEASE_BRANCH} and pulling latest changes...");
    run_git(&["checkout", RELEASE_BRANCH]);
    run_git(&["pull"]);

    let current = manifest_version();
    let version = next_version(parse_version(&current));
    let tag = format!("{TAG_PREFIX}{version}");

    println!("> Stamping {tag} (was {current})...");
    stamp_version(&current, &version);

    println!("> Verifying build state with cargo check...");
    run_cargo(&["check"]);

    let release_files = [MANIFEST, LOCKFILE];

    run_git(&[&["add"][..], &release_files].concat());

    if !git_succeeds(&[&["diff", "--cached", "--quiet", "--"][..], &release_files].concat()) {
        println!("> Committing {tag}...");
        run_git(&[&["commit", "-m", &tag, "--"][..], &release_files].concat());
    }

    println!("> Pushing {RELEASE_BRANCH} to remote...");
    run_git(&["push"]);

    println!("{tag} shipped! The rolling release workflow builds and publishes it.");
}

fn main() {
    let args: Vec<String> = env::args().skip(1).collect();

    match args.first().map(String::as_str) {
        None => ship(),
        Some(unknown) => {
            eprintln!("Unknown xtask command '{unknown}'. Use 'cargo ship'.");
            exit(2);
        }
    }
}
