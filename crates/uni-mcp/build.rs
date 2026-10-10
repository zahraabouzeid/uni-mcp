use std::path::Path;

fn main() {
    println!("cargo:rerun-if-changed=../../web/out");
    assert!(
        Path::new("../../web/out/index.html").exists(),
        "the setup page is not built yet: run `npm install` and `npm run build` in the `web` folder, then build again"
    );
}
