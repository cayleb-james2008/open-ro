use std::env;
use std::fs;
use std::path::Path;

fn main() {
    let manifest_dir = env::var("CARGO_MANIFEST_DIR").unwrap();
    let chunks_dir = Path::new(&manifest_dir).join("assets").join("chunks");
    println!("cargo:rerun-if-changed=assets/chunks");

    let mut entries: Vec<String> = Vec::new();
    if chunks_dir.is_dir() {
        if let Ok(rd) = fs::read_dir(&chunks_dir) {
            for ent in rd.flatten() {
                let p = ent.path();
                if p.is_file() {
                    if let Some(name) = p.file_name().and_then(|s| s.to_str()) {
                        if name.ends_with(".js") || name.ends_with(".css") {
                            println!("cargo:rerun-if-changed=assets/chunks/{name}");
                            entries.push(name.to_string());
                        }
                    }
                }
            }
        }
    }
    entries.sort();

    let mut out = String::from("pub fn get(name: &str) -> Option<&'static [u8]> {\n    match name {\n");
    for name in &entries {
        out.push_str(&format!(
            "        {name:?} => Some(include_bytes!(concat!(env!(\"CARGO_MANIFEST_DIR\"), \"/assets/chunks/{name}\"))),\n"
        ));
    }
    out.push_str("        _ => None,\n    }\n}\n");

    let out_dir = env::var("OUT_DIR").unwrap();
    fs::write(Path::new(&out_dir).join("ui_chunks.rs"), out).unwrap();
}
