use std::error::Error;

fn main() -> Result<(), Box<dyn Error>> {
    // Use vendored protoc so we don't depend on system protoc
    let protoc_path = protoc_bin_vendored::protoc_bin_path()
        .map_err(|e| format!("protoc-bin-vendored error: {}", e))?;
    std::env::set_var("PROTOC", protoc_path);

    tonic_build::configure()
        .build_server(true)
        .build_client(true)
        .out_dir(std::env::var("OUT_DIR").unwrap())
        .compile_protos(&["proto/gate.proto", "proto/game.proto"], &["proto"])?;

    println!("cargo:rerun-if-changed=proto/gate.proto");
    println!("cargo:rerun-if-changed=proto/game.proto");

    Ok(())
}
