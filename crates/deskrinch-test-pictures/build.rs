//! Makes the ordinary corpus (`src/ordinary.rs`) once, when the crate is
//! built: each picture to a file in `OUT_DIR`, and `ordinary.rs` there, which
//! names them and embeds them (`hostile::ordinary`). Encoding them takes
//! seconds unoptimised; made at run time, every test process that used them
//! paid that, and a property test with a timeout paid it inside its timed
//! cases, once in each forked child.
//!
//! The script reruns when the corpus's code or the committed fixtures
//! change; cargo reruns it when an encoder it uses changes.
#![expect(
    clippy::unwrap_used,
    reason = "a build script: a picture that cannot be made or written fails the build"
)]
#![expect(
    dead_code,
    unreachable_pub,
    reason = "the library's modules are included whole, as private modules of the script, and the corpus uses only some of what they offer"
)]

#[path = "src/case.rs"]
mod case;
#[path = "src/fixtures.rs"]
mod fixtures;
#[path = "src/gif.rs"]
mod gif;
#[path = "src/icc.rs"]
mod icc;
#[path = "src/jpeg.rs"]
mod jpeg;
#[path = "src/ordinary.rs"]
mod ordinary;
#[path = "src/png.rs"]
mod png;
#[path = "src/webp.rs"]
mod webp;

use std::fmt::Write as _;
use std::path::PathBuf;

/// What the script reads: the corpus and the code that makes it.
const INPUTS: &[&str] = &[
    "build.rs",
    "src/case.rs",
    "src/fixtures.rs",
    "src/gif.rs",
    "src/icc.rs",
    "src/jpeg.rs",
    "src/ordinary.rs",
    "src/png.rs",
    "src/webp.rs",
    "fixtures",
];

fn main() {
    for input in INPUTS {
        println!("cargo::rerun-if-changed={input}");
    }
    let out = PathBuf::from(std::env::var_os("OUT_DIR").unwrap());
    let dir = out.join("ordinary");
    std::fs::create_dir_all(&dir).unwrap();
    let mut table = String::from(
        "/// The ordinary corpus: each picture's name and bytes, made by the build script.\n\
         pub(super) const ORDINARY: &[(&str, &[u8])] = &[\n",
    );
    for (i, case) in ordinary::make().iter().enumerate() {
        let file = dir.join(format!("{i:02}.bin"));
        std::fs::write(&file, &case.bytes).unwrap();
        writeln!(
            table,
            "    ({:?}, include_bytes!({:?})),",
            case.name,
            file.to_str().unwrap()
        )
        .unwrap();
    }
    table.push_str("];\n");
    std::fs::write(out.join("ordinary.rs"), table).unwrap();
}
