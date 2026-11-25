use std::env;
use std::fs;
use std::path::Path;

fn main() {
    let manifest_dir = env::var("CARGO_MANIFEST_DIR").unwrap();
    let chunks_dir = Path::new(&manifest_dir).join("assets").join("chunks");
    println!("cargo:rerun-if-changed=assets/index.html.gz");
    println!("cargo:rerun-if-changed=assets/chunks");

    let mut entries: Vec<(String, String)> = Vec::new();
    if chunks_dir.is_dir() {
        if let Ok(rd) = fs::read_dir(&chunks_dir) {
            for ent in rd.flatten() {
                let p = ent.path();
                if p.is_file() {
                    if let Some(fname) = p.file_name().and_then(|s| s.to_str()) {
                        if let Some(logical) = fname.strip_suffix(".gz") {
                            println!("cargo:rerun-if-changed=assets/chunks/{fname}");
                            entries.push((logical.to_string(), fname.to_string()));
                        } else if fname.ends_with(".js") || fname.ends_with(".css") {
                            println!("cargo:rerun-if-changed=assets/chunks/{fname}");
                            entries.push((fname.to_string(), fname.to_string()));
                        }
                    }
                }
            }
        }
    }
    entries.sort();

    let mut out = String::from("pub fn get_gz(name: &str) -> Option<(&'static [u8], bool)> {\n    match name {\n");
    for (logical, file_name) in &entries {
        let is_gz = file_name.ends_with(".gz");
        out.push_str(&format!(
            "        {logical:?} => Some((include_bytes!(concat!(env!(\"CARGO_MANIFEST_DIR\"), \"/assets/chunks/{file_name}\")), {is_gz})),\n"
        ));
    }
    out.push_str("        _ => None,\n    }\n}\n");

    let out_dir = env::var("OUT_DIR").unwrap();
    fs::write(Path::new(&out_dir).join("ui_chunks.rs"), out).unwrap();
}
