use std::path::Path;

fn main() {
    let args: Vec<String> = std::env::args().collect();
    if args.len() < 2 {
        eprintln!("Usage: wat2wasm <input.wat> [output.wasm]");
        std::process::exit(1);
    }

    let input = &args[1];
    let output = if args.len() > 2 {
        args[2].clone()
    } else {
        input.replace(".wat", ".wasm")
    };

    let wat_content = std::fs::read_to_string(input).expect("Failed to read WAT file");
    let wasm_bytes = wat::parse_str(&wat_content).expect("Failed to parse WAT");

    std::fs::write(&output, &wasm_bytes).expect("Failed to write WASM file");
    println!("Compiled {} -> {} ({} bytes)", input, output, wasm_bytes.len());
}
