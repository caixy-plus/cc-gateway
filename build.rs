use std::env;
use std::path::Path;

fn main() {
    // Tell cargo to rerun if webui/dist changes
    println!("cargo:rerun-if-changed=webui/dist/index.html");

    let dist = Path::new("webui/dist");
    if !dist.exists() || !dist.join("index.html").exists() {
        let profile = env::var("PROFILE").unwrap_or_default();
        if profile == "release" {
            panic!(
                "\n\n\
                webui/dist/index.html not found.\n\n\
                To build with the embedded WebUI frontend:\n  \
                cd ../cc-gateway-webui && npm run build && \
                mkdir -p webui/dist && cp -r dist/* webui/dist/\n"
            );
        }
        eprintln!("\n============================================================");
        eprintln!("WARNING: webui/dist/index.html not found.");
        eprintln!("The dev binary will NOT include the WebUI frontend.");
        eprintln!();
        eprintln!("To embed the frontend:");
        eprintln!("  cd ../cc-gateway-webui && npm run build:embed");
        eprintln!("============================================================\n");
    }
}
