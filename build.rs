fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Compile protobuf files into Rust code
    tonic_build::configure()
        .build_server(true)
        .build_client(true)
        .out_dir("src/grpc") // Output directory for generated code
        .compile_protos(
            &[
                "proto/kv.proto",
                "proto/management.proto",
                "proto/raft.proto",
            ],
            &["proto"], // Include directory
        )?;

    // Tell Cargo to re-run this build script if any proto file changes
    println!("cargo:rerun-if-changed=proto/kv.proto");
    println!("cargo:rerun-if-changed=proto/management.proto");
    println!("cargo:rerun-if-changed=proto/raft.proto");

    Ok(())
}
