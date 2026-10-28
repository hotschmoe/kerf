use std::process::ExitCode;

mod pdf;
mod png;
mod sort;

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let Some((cmd, rest)) = args.split_first() else {
        eprintln!("usage: kvcheck <pdf|sort|json|png|raster|bench> ...");
        return ExitCode::from(2);
    };
    let ok = match cmd.as_str() {
        "pdf" => pdf::check(rest),
        "sort" => sort::check(rest),
        "png" => png::check(rest),
        _ => {
            eprintln!("kvcheck: unknown command {cmd}");
            false
        }
    };
    if ok { ExitCode::SUCCESS } else { ExitCode::FAILURE }
}
