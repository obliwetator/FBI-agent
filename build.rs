fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("cargo:rerun-if-changed=proto/helloworld.proto");
    println!("cargo:rerun-if-changed=build.rs");
    tonic_prost_build::compile_protos("proto/helloworld.proto")?;
    Ok(())
}
