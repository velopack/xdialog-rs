#![allow(missing_docs)]
use crate::model::DialogMessageRequest;
use std::sync::mpsc::SendError;

// #[derive(Debug, Clone)]
// pub struct NotInitializedError;

// impl fmt::Display for NotInitializedError {
//     fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
//         write!(f, "xdialog backend not initialized")
//     }
// }

// impl std::error::Error for NotInitializedError {}

#[derive(thiserror::Error, Debug)]
pub enum XDialogError {
    #[error("xdialog backend not initialized")]
    NotInitialized,
    #[error("xdialog send to backend failed: {0}")]
    SendFailed(SendError<DialogMessageRequest>),
    #[error("xdialog generic error: {0}")]
    GenericError(String),
}

// impl fmt::Display for XDialogError {
//     fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
//         match *self {
//             XDialogError::NotInitialized(ref err) => write!(f, "Initialization Error: {}", err),
//             XDialogError::SendFailed(ref err) => write!(f, "Send Error: {}", err),
//         }
//     }
// }

// impl std::error::Error for XDialogError {
//     fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
//         match *self {
//             XDialogError::NotInitialized(ref err) => Some(err),
//             XDialogError::SendFailed(ref err) => Some(err),
//         }
//     }
// }
