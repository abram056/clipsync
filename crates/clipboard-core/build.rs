fn main() {
    println!("cargo:rerun-if-changed=src/clipsync.udl");
    #[cfg(feature = "uniffi")]
    uniffi::generate_scaffolding("src/clipsync.udl").unwrap();
}
