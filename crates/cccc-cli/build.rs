#[path = "../cccc-core/build_support/mod.rs"]
mod source_id;

fn main() {
    source_id::emit(std::path::Path::new("../..")).expect("fingerprint CCCC application source");
}
