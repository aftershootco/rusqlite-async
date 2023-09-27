use crate::{errors, errors::Result, Connection, Message};
use core::panic::{RefUnwindSafe, UnwindSafe};
use rusqlite::{Connection as SqliteConnection, ToSql};

// type DelegateFn<T> = FnMut(&mut SqliteConnection) -> Result<T, rusqlite::Error> + Send;

impl Connection {
    pub async fn delegate_mut<T: Send + Sync + 'static>(
        &mut self,
        f: impl FnOnce(&mut SqliteConnection) -> Result<T, rusqlite::Error> + Send + 'static,
    ) -> Result<T, errors::Error> {
        // let query = f.into();
        // Now wrap it in a closure that will send the result back to the main thread
        let (rtx, rrx) = oneshot::channel();
        let f = Box::new(
            move |conn: &mut SqliteConnection| -> Result<(), errors::Error> {
                let res = f(conn).map_err(|e| errors::ErrorKind::Rusqlite(e).into());
                rtx.send(res).map_err(|_| errors::ErrorKind::Closed)?;
                Ok(())
            },
        );
        self.channel
            .send(Message::QueryMut(f))
            .map_err(|_| errors::ErrorKind::Closed)?;
        rrx.await.map_err(|_| errors::ErrorKind::Closed)?
    }

    pub async fn delegate<T: Send + Sync + 'static>(
        &self,
        f: impl FnOnce(&SqliteConnection) -> Result<T, rusqlite::Error> + Send + 'static,
    ) -> Result<T, errors::Error> {
        // Now wrap it in a closure that will send the result back to the main thread
        let (rtx, rrx) = oneshot::channel();
        let f = Box::new(
            move |conn: &SqliteConnection| -> Result<(), errors::Error> {
                let res = f(conn).map_err(|e| errors::ErrorKind::Rusqlite(e).into());
                rtx.send(res).map_err(|_| errors::ErrorKind::Closed)?;
                Ok(())
            },
        );
        self.channel
            .send(Message::Query(f))
            .map_err(|_| errors::ErrorKind::Closed)?;
        rrx.await.map_err(|_| errors::ErrorKind::Closed)?
    }
}

#[cfg(feature = "functions")]
impl crate::Connection {
    pub async fn create_scalar_function<F, T>(
        &self,
        fn_name: &'static str,
        n_arg: core::ffi::c_int,
        flags: rusqlite::functions::FunctionFlags,
        x_func: F,
    ) -> Result<()>
    where
        F: FnMut(&rusqlite::functions::Context<'_>) -> Result<T, rusqlite::Error>
            + Send
            + UnwindSafe
            + 'static,
        T: ToSql,
    {
        self.delegate(move |conn| conn.create_scalar_function(fn_name, n_arg, flags, x_func))
            .await
    }

    pub async fn create_aggregate_function<A, D, T>(
        &self,
        fn_name: &'static str,
        n_arg: core::ffi::c_int,
        flags: rusqlite::functions::FunctionFlags,
        aggr: D,
    ) -> Result<(), errors::Error>
    where
        A: RefUnwindSafe + UnwindSafe,
        D: rusqlite::functions::Aggregate<A, T> + 'static + Send,
        T: ToSql,
    {
        self.delegate(move |conn| conn.create_aggregate_function(fn_name, n_arg, flags, aggr))
            .await
    }

    #[cfg(feature = "window")]
    pub async fn create_window_function<A, W, T>(
        &self,
        fn_name: &'static str,
        n_arg: core::ffi::c_int,
        flags: rusqlite::functions::FunctionFlags,
        aggr: W,
    ) -> Result<(), errors::Error>
    where
        A: RefUnwindSafe + UnwindSafe,
        W: rusqlite::functions::WindowAggregate<A, T> + 'static + Send,
        T: ToSql,
    {
        self.delegate(move |conn| conn.create_window_function(fn_name, n_arg, flags, aggr))
            .await
    }

    pub async fn remove_function(
        &self,
        fn_name: &'static str,
        n_arg: core::ffi::c_int,
    ) -> Result<()> {
        self.delegate(move |conn| conn.remove_function(fn_name, n_arg))
            .await
    }
}
