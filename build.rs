fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-changed=assets/icon.ico");
    // The icon, and the version details Windows Installer goes by: without a version it may keep the old program
    // file during an upgrade. A local build can do without them; a release build can't.
    let mut res = winresource::WindowsResource::new();
    res.set_icon("assets/icon.ico").set("ProductName", "Startline").set("FileDescription", "Startline");
    if let Err(e) = res.compile() {
        if std::env::var_os("CI").is_some() {
            panic!("couldn't embed the icon and version: {e}");
        }
        println!("cargo:warning=building without the embedded icon and version: {e}");
    }
}
