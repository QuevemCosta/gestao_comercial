fn main() {
    slint_build::compile("ui/app.slint").expect("falha ao compilar a interface ui/app.slint");
    embed_windows_icon();
}

/// Incorpora o ícone e as informações do programa ao executável Windows.
/// Usa a variável de destino do Cargo, e não `cfg`, porque o build script roda no computador de origem.
fn embed_windows_icon() {
    println!("cargo:rerun-if-changed=assets/icon.ico");
    println!("cargo:rerun-if-changed=build.rs");

    let target_os = std::env::var("CARGO_CFG_TARGET_OS").unwrap_or_default();
    if target_os != "windows" {
        return;
    }

    let icon = std::path::Path::new("assets/icon.ico");
    if !icon.exists() {
        println!("cargo:warning=assets/icon.ico não encontrado; o executável será gerado sem ícone");
        return;
    }

    let mut resource = winres::WindowsResource::new();
    resource.set_icon("assets/icon.ico");
    resource.set("ProductName", "Gestão Comercial");
    resource.set("FileDescription", "Gestão Comercial");
    resource.set("OriginalFilename", "gestao_comercial.exe");

    if let Err(error) = resource.compile() {
        panic!("falha ao incorporar o ícone no executável: {error}");
    }
}