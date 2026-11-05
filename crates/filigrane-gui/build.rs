//! Pour une cible Windows (compilée sous Windows ou en croisé avec MinGW) : icône et
//! informations de version de l'exécutable.

fn main() {
    println!("cargo:rerun-if-changed=../../assets/icon/icon.ico");
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("windows") {
        return;
    }
    let mut resource = winresource::WindowsResource::new();
    resource
        .set_icon("../../assets/icon/icon.ico")
        .set("ProductName", "Filigrane PDF")
        .set("FileDescription", "Filigrane PDF")
        .set("LegalCopyright", "Florian Langlois");
    resource.compile().expect("compilation des ressources Windows");
}
