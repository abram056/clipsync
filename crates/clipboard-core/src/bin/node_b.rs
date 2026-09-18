fn main() {
    let config = clipboard_core::HarnessConfig::parse_args();
    clipboard_core::run_harness(config);
}
