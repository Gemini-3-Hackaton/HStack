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

#[tokio::main]
async fn main() {
    dotenvy::dotenv().ok();
    let db_url = std::env::var("DATABASE_URL").unwrap_or_else(|_| "sqlite:hstack_lite.db".to_string());
    
    let pool = SqlitePool::connect(&db_url).await.expect("Failed to connect to SQLite");
    
    // Minimal schema setup
    sqlx::query("CREATE TABLE IF NOT EXISTS users (id INTEGER PRIMARY KEY, first_name TEXT, password TEXT, created_at DATETIME)")
        .execute(&pool).await.unwrap();
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
    let hashed = bcrypt::hash(payload.password.unwrap_or_default(), bcrypt::DEFAULT_COST).unwrap();
    let id = sqlx::query("INSERT INTO users (first_name, password, created_at) VALUES (?, ?, ?)")
        .bind(&payload.first_name)
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
            first_name: payload.first_name,
            last_name: payload.last_name.unwrap_or_default(),
            created_at: Utc::now(),
        }
    }))
}

async fn login(State(state): State<AppState>, Json(payload): Json<UserLogin>) -> Result<Json<AuthResponse>, StatusCode> {
    let row = sqlx::query("SELECT id, first_name, password FROM users WHERE first_name = ?")
        .bind(&payload.first_name)
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
            created_at: Utc::now(),
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
    use super::*;

    #[test]
    fn test_bcrypt_logic() {
        let password = "password123";
        let hashed = bcrypt::hash(password, bcrypt::DEFAULT_COST).unwrap();
        assert!(bcrypt::verify(password, &hashed).unwrap());
    }
}
