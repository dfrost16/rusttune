use std::env;
use std::fs;
use std::path::{Path, PathBuf};

fn collect_files(root: &Path, dir: &Path, out: &mut Vec<(String, PathBuf)>) {
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let p = entry.path();
        if p.is_dir() {
            collect_files(root, &p, out);
        } else if p.is_file() {
            if let Ok(rel) = p.strip_prefix(root) {
                let web_path = format!("/{}", rel.to_string_lossy().replace('\\', "/"));
                let abs = fs::canonicalize(&p).unwrap_or(p);
                out.push((web_path, abs));
            }
        }
    }
}

fn mime_for(path: &str) -> &'static str {
    if path.ends_with(".html") {
        "text/html; charset=utf-8"
    } else if path.ends_with(".js") {
        "application/javascript; charset=utf-8"
    } else if path.ends_with(".css") {
        "text/css; charset=utf-8"
    } else if path.ends_with(".json") || path.ends_with(".webmanifest") {
        "application/json; charset=utf-8"
    } else if path.ends_with(".png") {
        "image/png"
    } else if path.ends_with(".svg") {
        "image/svg+xml"
    } else {
        "application/octet-stream"
    }
}

fn main() {
    let config = slint_build::CompilerConfiguration::new()
        .embed_resources(slint_build::EmbedResourcesKind::EmbedFiles);
    slint_build::compile_with_config("ui/app.slint", config).unwrap();

    let dist_dir = PathBuf::from("../webapp/dist");
    println!("cargo:rerun-if-changed=../webapp/dist");

    let mut files = Vec::new();
    if dist_dir.is_dir() {
        collect_files(&dist_dir, &dist_dir, &mut files);
        files.sort_by(|a, b| a.0.cmp(&b.0));
    }

    let out_dir = PathBuf::from(env::var("OUT_DIR").unwrap());
    let mut code = String::from(
        "pub fn get_webapp_asset(path: &str) -> Option<(&'static str, &'static [u8])> {\n    match path {\n",
    );
    for (web_path, abs) in &files {
        let abs_str = abs.to_string_lossy().replace('\\', "/");
        let mime = mime_for(web_path);
        code.push_str(&format!(
            "        {web_path:?} => Some(({mime:?}, include_bytes!({abs_str:?}))),\n"
        ));
    }
    code.push_str("        _ => None,\n    }\n}\n");
    fs::write(out_dir.join("webapp_assets.rs"), code).unwrap();
}
