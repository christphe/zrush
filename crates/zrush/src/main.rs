#![forbid(unsafe_code)]

fn main() -> std::process::ExitCode {
    match zrush::cli::main() {
        Ok(()) => std::process::ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("zrush: {e}");
            std::process::ExitCode::FAILURE
        }
    }
}
