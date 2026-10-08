//! The program the user edits in. Only programs that mark an open document with a lock file
//! are supported: that file is how a session tells that the document was closed.

use std::io;
use std::path::Path;

mod libreoffice;
mod msoffice;

pub use libreoffice::{LibreOffice, Prompts, PromptsError};
pub use msoffice::MsOffice;

pub trait Editor: Send + Sync {
    /// The name the user knows it by.
    fn name(&self) -> &str;
    /// Open path; view: read-only. Returns once the program is started.
    fn launch(&self, path: &Path, view: bool) -> io::Result<()>;
    /// True while path is open, judged by the program's lock file next to it.
    fn is_open(&self, path: &Path) -> bool;
    /// True while the program runs at all; used when its lock file never appeared.
    fn running(&self) -> bool;
}

/// True if a supported editor has path open.
pub fn open_in_any(path: &Path) -> bool {
    LibreOffice::new().is_open(path) || MsOffice.is_open(path)
}
