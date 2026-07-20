use diesel::connection::SimpleConnection;
use diesel::r2d2::{ConnectionManager, CustomizeConnection};
use diesel::sqlite::SqliteConnection;

use crate::error::AppError;

pub type Pool = diesel::r2d2::Pool<ConnectionManager<SqliteConnection>>;

#[derive(Debug)]
struct SqlitePragmas;

impl CustomizeConnection<SqliteConnection, diesel::r2d2::Error> for SqlitePragmas {
    fn on_acquire(&self, conn: &mut SqliteConnection) -> Result<(), diesel::r2d2::Error> {
        conn.batch_execute("PRAGMA foreign_keys = ON; PRAGMA busy_timeout = 5000; PRAGMA journal_mode = WAL;")
            .map_err(diesel::r2d2::Error::QueryError)
    }
}

pub fn init_pool(database_url: &str) -> Pool {
    let manager = ConnectionManager::<SqliteConnection>::new(database_url);
    diesel::r2d2::Pool::builder()
        .max_size(4)
        .connection_customizer(Box::new(SqlitePragmas))
        .build(manager)
        .expect("failed to create db pool")
}

/// Run a blocking Diesel closure on the blocking thread pool.
pub async fn run<T, F>(pool: &Pool, f: F) -> Result<T, AppError>
where
    T: Send + 'static,
    F: FnOnce(&mut SqliteConnection) -> Result<T, AppError> + Send + 'static,
{
    let pool = pool.clone();
    tokio::task::spawn_blocking(move || {
        let mut conn = pool.get().map_err(|e| AppError::Internal(e.to_string()))?;
        f(&mut conn)
    })
    .await?
}
