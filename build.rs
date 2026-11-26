fn main() {
    if std::env::var_os("CARGO_CFG_WINDOWS").is_some() {
        let mut res = winresource::WindowsResource::new();
        res.set_icon("logo.ico");
        res.set("ProductName", "GTA VI RPC");
        res.set("FileDescription", "Discord Rich Presence for GTA VI on PC");
        if let Err(e) = res.compile() {
            println!("cargo:warning=icon not embedded: {e}");
        }
    }
    println!("cargo:rerun-if-changed=logo.ico");
}
