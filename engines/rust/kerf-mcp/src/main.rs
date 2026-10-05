fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if let Err(e) = kerf_mcp::run_cli(&args) {
        eprintln!("kerf-mcp: {}", e);
        std::process::exit(1);
    }
}
