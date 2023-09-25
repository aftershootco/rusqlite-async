use thiserror::Error;

use crate::BoxedError;
pub type Result<T, E = Error> = core::result::Result<T, E>;

#[derive(Debug, Error)]
#[error(transparent)]
pub struct Error {
    kind: ErrorKind,
}

#[derive(Debug, Error)]
pub enum ErrorKind {
    // #[error(transparent)]
    // Io(#[from] std::io::Error),
    #[error(transparent)]
    Rusqlite(#[from] rusqlite::Error),
    #[error(transparent)]
    FlumeRecv(#[from] flume::RecvError),
    #[error(transparent)]
    Other(#[from] BoxedError<'static>),
    #[error("Database Connection Closed")]
    Closed,
}

impl<E> From<E> for Error
where
    ErrorKind: From<E>,
{
    #[track_caller]
    fn from(e: E) -> Self {
        Self {
            kind: From::from(e),
        }
    }
}

impl Error {
    pub fn kind(&self) -> &ErrorKind {
        &self.kind
    }

    pub fn downcast_ref<T: std::error::Error + 'static>(&self) -> Option<&T> {
        self.kind.downcast_ref()
    }
}

impl ErrorKind {
    pub fn downcast_ref<T: std::error::Error + 'static>(&self) -> Option<&T> {
        match self {
            Self::Rusqlite(_) => None,
            Self::FlumeRecv(_) => None,
            Self::Other(e) => e.downcast_ref(),
            Self::Closed => None,
        }
    }
}
