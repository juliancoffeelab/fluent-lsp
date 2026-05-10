use std::path::PathBuf;

use anyhow::Result;
use bench::{CommonArgs, build_archive_bytes};

fn main() -> Result<()> {
    let args = CommonArgs::parse();
    let bytes = build_archive_bytes(&args)?;
    let output = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("data/prebuilt-bench-snapshot.rkyv");
    std::fs::write(output, bytes)?;
    Ok(())
}
