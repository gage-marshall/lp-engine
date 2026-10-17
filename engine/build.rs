//! Windows builds: the program's icon and version information as resources (the icon from tools/package/icon).
fn main() {
    println!("cargo:rerun-if-changed=../tools/package/icon/icon.ico");
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") {
        let mut r = winresource::WindowsResource::new();
        r.set_icon("../tools/package/icon/icon.ico");
        r.set("ProductName", "LovePlus+");
        r.set("FileDescription", "LovePlus+");
        r.set("LegalCopyright", "Copyright (c) 2026 gage-marshall (MIT). LovePlus is a trademark of Konami Digital Entertainment.");
        if let Err(e) = r.compile() {
            println!("cargo:warning=no Windows resources: {e}");
        }
    }
}
