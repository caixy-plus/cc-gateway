use std::env;
use std::path::Path;

fn main() {
    // Tell cargo to rerun if webui/dist changes
    println!("cargo:rerun-if-changed=webui/dist/index.html");

    // Only check in release mode to avoid breaking dev builds
    let profile = env::var("PROFILE").unwrap_or_default();
    if profile == "release" {
        let dist = Path::new("webui/dist");
        if !dist.exists() || !dist.join("index.html").exists() {
            eprintln!("\n============================================================");
            eprintln!("ERROR: webui/dist/index.html not found.");
            eprintln!("");
            eprintln!("To build with the embedded WebUI frontend, run one of:");
            eprintln!("  1. Local:  cd ../cc-gateway-webui && npm run build:embed");
            eprintln!("  2. CI:     The GitHub Actions workflow handles this automatically.");
            eprintln!("============================================================\n");
            // Don't panic — let the build continue with a fallback.
            // The ui.rs handler will serve a placeholder if dist is missing.
        }
    }
}
