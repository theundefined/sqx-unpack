use sqx_unpack::extract;
use sqx_unpack::header;
use std::path::PathBuf;

fn usage() -> ! {
    eprintln!("usage: sqx l ARCHIVE\n       sqx t ARCHIVE\n       sqx x ARCHIVE [DIR]");
    std::process::exit(2)
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match args.first().map(String::as_str) {
        Some("-h" | "--help") => {
            println!("usage: sqx l ARCHIVE\n       sqx t ARCHIVE\n       sqx x ARCHIVE [DIR]");
            return;
        }
        Some("-V" | "--version") => {
            println!("sqx {}", env!("CARGO_PKG_VERSION"));
            return;
        }
        _ => {}
    }
    if args.len() < 2 {
        usage();
    }
    let cmd = args[0].as_str();
    let data = std::fs::read(&args[1]).unwrap_or_else(|e| {
        eprintln!("{}: {e}", args[1]);
        std::process::exit(1)
    });
    let entries = header::parse(&data).unwrap_or_else(|e| {
        eprintln!("error: {e}");
        std::process::exit(1)
    });
    match cmd {
        "l" => {
            for e in entries {
                println!("{:>12} {:>12} m{} {:08x} {}", e.usize, e.csize, e.method, e.crc_field, e.name);
            }
        }
        "t" | "x" => {
            let dir = if cmd == "x" { Some(PathBuf::from(args.get(2).map(String::as_str).unwrap_or("."))) } else { None };
            if extract::run(&data, &entries, dir.as_deref()) > 0 {
                std::process::exit(1);
            }
        }
        _ => usage(),
    }
}
