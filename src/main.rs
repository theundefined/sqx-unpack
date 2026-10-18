mod header;

fn main() {
    let args: Vec<String> = std::env::args().collect();
    if args.len() != 3 || args[1] != "l" {
        eprintln!("usage: sqx l ARCHIVE   (x and t: not implemented yet)");
        std::process::exit(2);
    }
    let data = std::fs::read(&args[2]).unwrap_or_else(|e| {
        eprintln!("{}: {e}", args[2]);
        std::process::exit(1)
    });
    match header::parse(&data) {
        Ok(es) => {
            for e in es {
                println!("{:>12} {:>12} m{} {:08x} {}", e.usize, e.csize, e.method, e.crc_field, e.name);
            }
        }
        Err(e) => {
            eprintln!("error: {e}");
            std::process::exit(1)
        }
    }
}
