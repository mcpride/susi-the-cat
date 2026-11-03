fn main() -> std::process::ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let code = susi_cli::run(&args);
    std::process::ExitCode::from(u8::try_from(code).unwrap_or(1))
}
