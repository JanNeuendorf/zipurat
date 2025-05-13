use std::path::Path;

mod archiver;
mod index;
mod utils;
fn main() {
    archiver::build_archive(
        &Path::new("target"),
        &mut utils::GenericFile::Local(std::fs::File::create("testing/test.zipurat.zip").unwrap()),
        &Path::new("testing/testkey.age"),
        4,
    )
    .unwrap();
}
