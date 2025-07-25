use std::path::Path;

use sqlx::{sqlite::SqlitePoolOptions, SqlitePool};
use tokio::sync::OnceCell;
use anyhow::{anyhow, Error};
use sqlx::migrate::Migrator;

use crate::utils::config::get_config;

pub struct Database {
    pool: SqlitePool
}

impl Database {
    pub async fn init_application_db(db_url: &str) -> Result<Self, Error> {
        let file_path = db_url.strip_prefix("sqlite:").unwrap_or(db_url);

        if !Path::new(file_path).exists() {
            return Err(anyhow!("Database path is invalid or file does not exist: {}", file_path));
        }

        let pool = SqlitePoolOptions::new()
            .max_connections(5)
            .connect(db_url)
            .await?;
        

        let db = Database {pool};
        db.run_migrations().await?;

        Ok(db)
    }

    pub fn get_pool(&self) -> &SqlitePool {
        &self.pool
    }

    pub async fn run_migrations(&self) -> Result<(), Error> {
        // TODO: Add migrations path to Config!
        let migrations = Migrator::new(Path::new("./data/db/migrations")).await?;
        migrations.run(&self.pool).await?;

        Ok(())
    }
}

pub async fn get_application_db() -> Result<&'static Database, Error> {
    static DB_INSTANCE: OnceCell<Result<Database, String>> = OnceCell::const_new();

    let result = DB_INSTANCE.get_or_init(|| async {
        let config = match get_config() {
            Ok(config) => config,
            Err(err) => return Err(err.to_string()),
        };
        
        let db_path = match config.database.path.to_str() {
            Some(path) => path,
            None => return Err("Failed to parse configs DB path into string!".to_string()),
        };

        let db_url = format!("sqlite:{}", db_path);
        
        match Database::init_application_db(&db_url).await {
            Ok(db) => Ok(db),
            Err(e) => Err(e.to_string()),
        }
    }).await;

    match result {
        Ok(db) => Ok(db),
        Err(msg) => Err(anyhow!("{}", msg)),
    }
}