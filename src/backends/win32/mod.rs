#[cfg(windows)]
pub mod taskdialog;

#[cfg(not(feature = "win32-direct"))]
mod backend;
#[cfg(not(feature = "win32-direct"))]
pub use backend::Win32Backend;
