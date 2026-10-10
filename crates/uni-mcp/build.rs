use std::path::Path;

fn main() {
    println!("cargo:rerun-if-changed=../../web/out");
    assert!(
        Path::new("../../web/out/index.html").exists(),
        "the dashboard is not built yet: run `npm install` and `npm run build` in the `web` folder, then build again"
    );

    println!("cargo:rerun-if-changed=assets/unimcp.ico");
    if std::env::var_os("CARGO_CFG_WINDOWS").is_some() {
        winresource::WindowsResource::new()
            .set_icon("assets/unimcp.ico")
            .set("ProductName", "uniMCP")
            .set("FileDescription", "uniMCP")
            .compile()
            .expect("the program icon can be embedded");
    }
}
