use sqlx::PgPool;

mod issuance;
mod links;

#[derive(Clone)]
pub struct DatabaseMcGuildLinkRepository {
    pool: PgPool,
}

impl DatabaseMcGuildLinkRepository {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }
}
