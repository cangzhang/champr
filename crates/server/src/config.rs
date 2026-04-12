#[derive(Debug, Clone)]
pub struct Config {
    pub addr: String,
    pub database_url: String,
    pub jwt_secret: String,
    pub bootstrap_admin_email: Option<String>,
    pub bootstrap_admin_password: Option<String>,
}

impl Config {
    pub fn from_env() -> Self {
        Self {
            addr: std::env::var("SERVER_ADDR").unwrap_or_else(|_| "0.0.0.0:3030".to_string()),
            database_url: std::env::var("DATABASE_URL")
                .unwrap_or_else(|_| "sqlite://data/champr.db".to_string()),
            jwt_secret: std::env::var("JWT_SECRET")
                .unwrap_or_else(|_| "change-me-in-production".to_string()),
            bootstrap_admin_email: std::env::var("BOOTSTRAP_ADMIN_EMAIL").ok(),
            bootstrap_admin_password: std::env::var("BOOTSTRAP_ADMIN_PASSWORD").ok(),
        }
    }
}
