// Minimal public server implementation.
// Review docs/public-private-contract.md before adding backend complexity that belongs in the private server.
use hstack_core::api_models::{UserCreate, UserLogin, AuthResponse, UserDTO, CreateTaskPayload};
use axum::{
    routing::{get, post},
    extract::{State},
    http::StatusCode,
    Json, Router,
};
use sqlx::{SqlitePool, Row};
use std::net::SocketAddr;
use chrono::Utc;
use uuid::Uuid;

#[derive(Clone)]
struct AppState {
    db: SqlitePool,
}

fn required_trimmed(value: &str) -> Option<String> {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        None
    } else {
        Some(trimmed.to_string())
    }
}

fn ensure_password_present(password: &str) -> Result<(), StatusCode> {
    if password.trim().is_empty() {
        Err(StatusCode::BAD_REQUEST)
    } else {
        Ok(())
    }
}

#[tokio::main]
async fn main() {
    dotenvy::dotenv().ok();
    let db_url = std::env::var("DATABASE_URL").unwrap_or_else(|_| "sqlite:hstack_lite.db".to_string());
    
    let pool = SqlitePool::connect(&db_url).await.expect("Failed to connect to SQLite");
    
    // Minimal schema setup
    sqlx::query("CREATE TABLE IF NOT EXISTS users (id INTEGER PRIMARY KEY, first_name TEXT, email TEXT, password TEXT, created_at DATETIME)")
        .execute(&pool).await.unwrap();
    let _ = sqlx::query("ALTER TABLE users ADD COLUMN email TEXT")
        .execute(&pool)
        .await;
    sqlx::query("CREATE TABLE IF NOT EXISTS tasks (id TEXT PRIMARY KEY, userid INTEGER, type TEXT, payload TEXT, status TEXT, created_at DATETIME)")
        .execute(&pool).await.unwrap();

    let state = AppState { db: pool };

    let app = Router::new()
        .route("/api/auth/register", post(register))
        .route("/api/auth/login", post(login))
        .route("/api/tasks", get(get_tasks).post(create_task))
        .with_state(state);

    let addr = SocketAddr::from(([127, 0, 0, 1], 8001));
    println!("HStack Lite Server listening on {}", addr);
    let listener = tokio::net::TcpListener::bind(addr).await.unwrap();
    axum::serve(listener, app).await.unwrap();
}

async fn register(State(state): State<AppState>, Json(payload): Json<UserCreate>) -> Result<Json<AuthResponse>, StatusCode> {
    let first_name = required_trimmed(&payload.first_name).ok_or(StatusCode::BAD_REQUEST)?;
    let email = required_trimmed(&payload.email).ok_or(StatusCode::BAD_REQUEST)?;
    let last_name = payload.last_name.unwrap_or_default().trim().to_string();
    ensure_password_present(&payload.password)?;

    let hashed = bcrypt::hash(&payload.password, bcrypt::DEFAULT_COST)
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    let id = sqlx::query("INSERT INTO users (first_name, email, password, created_at) VALUES (?, ?, ?, ?)")
        .bind(&first_name)
        .bind(&email)
        .bind(&hashed)
        .bind(Utc::now())
        .execute(&state.db)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
        .last_insert_rowid();

    Ok(Json(AuthResponse {
        token: "lite_token_no_jwt_verification_needed".to_string(),
        user: UserDTO {
            id,
            first_name,
            last_name,
            email: Some(email),
            created_at: Utc::now(),
            auth_identities: Vec::new(),
        }
    }))
}

async fn login(State(state): State<AppState>, Json(payload): Json<UserLogin>) -> Result<Json<AuthResponse>, StatusCode> {
    let email = required_trimmed(&payload.email).ok_or(StatusCode::BAD_REQUEST)?;
    ensure_password_present(&payload.password)?;

    let row = sqlx::query("SELECT id, first_name, email, password FROM users WHERE lower(email) = lower(?)")
        .bind(&email)
        .fetch_optional(&state.db)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
        .ok_or(StatusCode::UNAUTHORIZED)?;

    let db_pass: String = row.get("password");
    if !bcrypt::verify(&payload.password, &db_pass).unwrap_or(false) {
        return Err(StatusCode::UNAUTHORIZED);
    }

    Ok(Json(AuthResponse {
        token: "lite_token".to_string(),
        user: UserDTO {
            id: row.get("id"),
            first_name: row.get("first_name"),
            last_name: "".to_string(),
            email: row.try_get("email").ok(),
            created_at: Utc::now(),
            auth_identities: Vec::new(),
        }
    }))
}

async fn get_tasks(State(state): State<AppState>) -> Result<Json<Vec<serde_json::Value>>, StatusCode> {
    let rows = sqlx::query("SELECT id, userid, type, payload, status, created_at FROM tasks ORDER BY created_at ASC")
        .fetch_all(&state.db)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    let mut tasks = Vec::new();
    for row in rows {
        let payload_str: String = row.get("payload");
        tasks.push(serde_json::from_str(&payload_str).unwrap_or(serde_json::json!({})));
    }
    
    Ok(Json(tasks))
}

async fn create_task(State(state): State<AppState>, Json(payload): Json<CreateTaskPayload>) -> Result<Json<serde_json::Value>, StatusCode> {
    let id = Uuid::new_v4().to_string();
    let payload_json = serde_json::to_string(&payload.payload).unwrap_or_default();
    
    sqlx::query("INSERT INTO tasks (id, userid, type, payload, status, created_at) VALUES (?, ?, ?, ?, ?, ?)")
        .bind(&id)
        .bind(1) // Mock user for Lite
        .bind(&payload.r#type)
        .bind(&payload_json)
        .bind(&payload.status)
        .bind(Utc::now())
        .execute(&state.db)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    Ok(Json(serde_json::json!({ "id": id, "status": "created" })))
}

#[cfg(test)]
mod tests {
    use super::{ensure_password_present, required_trimmed};

    #[test]
    fn test_bcrypt_logic() {
        let password = "password123";
        let hashed = bcrypt::hash(password, bcrypt::DEFAULT_COST).unwrap();
        assert!(bcrypt::verify(password, &hashed).unwrap());
    }

    #[test]
    fn rejects_blank_credentials() {
        assert_eq!(required_trimmed("  "), None);
        assert!(ensure_password_present(" ").is_err());
    }
}
