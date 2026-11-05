//! Directories the tests make, each with an explicit mode (0700), never the
//! umask's. The store refuses a state directory group or others can
//! use, so a test that left the mode to the umask passed only where the
//! umask was 077. `checks/dir_modes.rs` refuses any other way to make one.

use std::fs::DirBuilder;
use std::io;
use std::os::unix::fs::DirBuilderExt;
use std::path::Path;

/// Makes `dir` (0700). Fails if it exists.
pub fn private_dir(dir: impl AsRef<Path>) -> io::Result<()> {
    DirBuilder::new().mode(0o700).create(dir)
}

/// Makes `dir` and any missing parents, each 0700. A directory that exists
/// already keeps its mode.
pub fn private_dir_all(dir: impl AsRef<Path>) -> io::Result<()> {
    DirBuilder::new().recursive(true).mode(0o700).create(dir)
}
