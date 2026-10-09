//! Embed the web console and generated firmware emulator/update artifacts.
//! Release binaries serve the exact build tree without a runtime asset folder.
use std::{
    env, fs,
    path::{Path, PathBuf},
};

fn collect(root: &Path, folder: &Path, output: &mut Vec<PathBuf>, total: &mut u64) {
    let mut entries = fs::read_dir(folder)
        .expect("Read collector web assets")
        .map(|e| e.expect("Read web asset entry").path())
        .collect::<Vec<_>>();
    entries.sort();
    for path in entries {
        let meta = fs::symlink_metadata(&path).expect("Read web asset metadata");
        assert!(
            !meta.file_type().is_symlink(),
            "Web assets cannot be symlinks"
        );
        let name = path.file_name().unwrap().to_string_lossy();
        if name.starts_with('.') {
            continue;
        }
        if meta.is_dir() {
            collect(root, &path, output, total);
        } else if meta.is_file() {
            let relative = path.strip_prefix(root).unwrap();
            let limit = if relative == Path::new("firmware/source-relink.tar.gz") {
                32 * 1024 * 1024
            } else {
                8 * 1024 * 1024
            };
            assert!(meta.len() <= limit, "Web asset exceeds its size limit");
            *total += meta.len();
            assert!(*total <= 64 * 1024 * 1024, "Web assets exceed 64 MiB");
            assert!(
                path.strip_prefix(root).unwrap().to_string_lossy().len() <= 240,
                "Web asset path too long"
            );
            output.push(path);
        }
    }
}
fn main() {
    let root = PathBuf::from(env::var("CARGO_MANIFEST_DIR").unwrap()).join("../collector-web");
    for path in ["index.html", "assets", "emulator", "firmware"] {
        println!("cargo:rerun-if-changed={}", root.join(path).display());
    }
    let mut files = Vec::new();
    if root.is_dir() {
        let index = root.join("index.html");
        if index.is_file() {
            let meta = fs::symlink_metadata(&index).expect("Read console entry");
            assert!(
                !meta.file_type().is_symlink(),
                "Web assets cannot be symlinks"
            );
            assert!(meta.len() <= 8 * 1024 * 1024, "Web asset exceeds 8 MiB");
            files.push(index);
        }
        let mut total = 0;
        for directory in ["assets", "emulator", "firmware"] {
            let path = root.join(directory);
            if path.exists() {
                let meta = fs::symlink_metadata(&path).expect("Read web asset folder");
                assert!(
                    !meta.file_type().is_symlink() && meta.is_dir(),
                    "Web asset folders must be directories"
                );
                collect(&root, &path, &mut files, &mut total);
            }
        }
    }
    let mut code = String::from(
        "pub fn asset(path: &str) -> Option<(&'static [u8], &'static str)> { match path {\n",
    );
    for file in files {
        let relative = file
            .strip_prefix(&root)
            .unwrap()
            .to_string_lossy()
            .replace('\\', "/");
        let mime = match file.extension().and_then(|e| e.to_str()).unwrap_or("") {
            "html" => "text/html; charset=utf-8",
            "js" | "mjs" => "text/javascript; charset=utf-8",
            "css" => "text/css; charset=utf-8",
            "json" => "application/json; charset=utf-8",
            "wasm" => "application/wasm",
            "png" => "image/png",
            "svg" => "image/svg+xml",
            "ico" => "image/x-icon",
            "woff2" => "font/woff2",
            _ => "application/octet-stream",
        };
        let route = if relative == "index.html" {
            "/".to_string()
        } else {
            format!("/{relative}")
        };
        code.push_str(&format!(
            "{route:?} => Some((include_bytes!({:?}), {mime:?})),\n",
            file.canonicalize().unwrap().to_string_lossy()
        ));
    }
    code.push_str("_ => None, } }\n");
    fs::write(
        PathBuf::from(env::var("OUT_DIR").unwrap()).join("web_assets.rs"),
        code,
    )
    .unwrap();
}
