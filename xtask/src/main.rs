#![forbid(unsafe_code)]

mod file_sizes;
mod recipe_check;
mod simdoc;

fn main() {
    let args: Vec<String> = std::env::args().collect();
    if let Err(err) = run(args) {
        eprintln!("{err}");
        std::process::exit(1);
    }
}

fn run(args: Vec<String>) -> Result<(), String> {
    let program = args.first().map(String::as_str).unwrap_or("xtask");
    match args.get(1).map(String::as_str) {
        Some("simdoc") => simdoc::run(args),
        Some("check-file-sizes") => file_sizes::run(&args),
        Some("check-recipes") => recipe_check::run(&args),
        _ => Err(format!(
            "usage: {program} <simdoc [--check]|check-file-sizes|check-recipes>"
        )),
    }
}
