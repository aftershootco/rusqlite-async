//! A rusqlite wrapper to allow it to be used in async contexts
//!
//! This is done by spwaning a tread with a SqliteConnection and sending queries to it
//! using a flume channel and making sure that no reference / mutable reference to the
//! SqliteConnection is passed to any other thread

mod delegate;
mod errors;
use rusqlite::Connection as SqliteConnection;
use std::path::Path;
use std::thread::JoinHandle;

type BoxedError<'a> = Box<dyn std::error::Error + Send + Sync + 'a>;
type BoxedQuery<'a, T = (), E = BoxedError<'static>> =
    Box<dyn FnOnce(&SqliteConnection) -> Result<T, E> + Send + 'a>;
type BoxedQueryMut<'a, T = (), E = BoxedError<'static>> =
    Box<dyn FnOnce(&mut SqliteConnection) -> Result<T, E> + Send + 'a>;

/// A wrapper around rusqlite::Connection that allows it to be used in async
pub struct Connection {
    handle: Option<JoinHandle<Result<(), errors::Error>>>,
    channel: flume::Sender<Message>,
}

impl Drop for Connection {
    fn drop(&mut self) {
        // Possible that this will fail if the thread panics
        if let Err(e) = self.channel.send(Message::Close) {
            tracing::error!("Error sending close message: {:?}", e);
        }

        let Some(handle) = self.handle.take() else {
            tracing::error!("Thread handle not found");
            return;
        };
        if let Err(e) = handle.join() {
            tracing::error!("Error joining thread: {:?}", e);
        }
    }
}

pub(crate) enum Message {
    Query(BoxedQuery<'static, (), errors::Error>),
    QueryMut(BoxedQueryMut<'static, (), errors::Error>),
    Close,
}

impl Connection {
    /// Run a query on the database
    /// Takes any closure that can takes a &mut rusqlite::Connection and returns a Result<T, Box<dyn Error>>
    /// You can downcast the error to your original variant using downcast_ref method on the the
    /// returned error
    pub async fn run<T: Send + Sync + 'static>(
        &mut self,
        f: impl FnOnce(&SqliteConnection) -> Result<T, BoxedError<'static>> + Send + 'static,
    ) -> Result<T, errors::Error> {
        // Now wrap it in a closure that will send the result back to the main thread
        let (rtx, rrx) = oneshot::channel();
        let f = Box::new(
            move |conn: &mut SqliteConnection| -> Result<(), errors::Error> {
                let res = f(conn).map_err(|e| errors::ErrorKind::Other(e).into());
                rtx.send(res).map_err(|_| errors::ErrorKind::Closed)?;
                Ok(())
            },
        );

        self.channel
            .send(Message::QueryMut(f))
            .map_err(|_| errors::ErrorKind::Closed)?;

        rrx.await.map_err(|_| errors::ErrorKind::Closed)?
    }

    /// Open a connection to a sqlite database
    pub fn open(path: impl AsRef<Path>) -> Result<Self, rusqlite::Error> {
        let (tx, rx) = flume::unbounded::<Message>();
        let path = path.as_ref().to_owned();
        let handle = std::thread::spawn(move || -> Result<(), errors::Error> {
            let mut conn = rusqlite::Connection::open(path)?;
            for msg in rx.into_iter() {
                match msg {
                    Message::Close => break,
                    Message::Query(wrapped_query) => {
                        wrapped_query(&conn)?;
                    }
                    Message::QueryMut(wrapped_query) => {
                        wrapped_query(&mut conn)?;
                    }
                }
            }
            Ok(())
        });
        Ok(Self {
            handle: Some(handle),
            channel: tx,
        })
    }

    /// Takes a function / closure that returns a rusqlite::Connection
    pub fn open_with(
        f: impl FnOnce() -> Result<SqliteConnection, errors::Error> + Send + 'static,
    ) -> Result<Self, errors::Error> {
        let (tx, rx) = flume::unbounded::<Message>();
        let handle = std::thread::spawn(move || -> Result<(), errors::Error> {
            let mut conn = f()?;
            for msg in rx.into_iter() {
                match msg {
                    Message::Close => break,
                    Message::Query(wrapped_query) => {
                        wrapped_query(&mut conn)?;
                    }
                    Message::QueryMut(wrapped_query) => {
                        wrapped_query(&mut conn)?;
                    }
                }
            }
            Ok(())
        });

        Ok(Self {
            handle: Some(handle),
            channel: tx,
        })
    }
}
