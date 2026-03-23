#![deny(clippy::unwrap_used, clippy::expect_used)]

// Minimal public server implementation.
// Review docs/public-private-contract.md before adding backend complexity that belongs in the private server.
use hstack_core::api_models::{AuthResponse, CreateTicketPayload, UserCreate, UserDTO, UserLogin};
use axum::{
    routing::{get, post},
    extract::{Query, State},
    http::StatusCode,
    Json, Router,
};
use serde::Deserialize;
use sqlx::{SqlitePool, Row};
use std::net::SocketAddr;
use chrono::Utc;
use serde::Serialize;
use uuid::Uuid;
use zxcvbn::{zxcvbn, Score};

const MIN_PASSWORD_LENGTH: usize = 12;
const MIN_PASSWORD_SCORE: Score = Score::Three;

#[derive(serde::Serialize)]
struct TicketDto {
    id: String,
    userid: i64,
    r#type: String,
    payload: serde_json::Value,
    status: String,
    created_at: String,
}

#[derive(Debug, Default, Deserialize)]
struct TicketQuery {
    userid: Option<i64>,
}

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

fn validate_new_password(password: &str, user_inputs: &[&str]) -> Result<(), StatusCode> {
    let trimmed = password.trim();
    if trimmed.len() < MIN_PASSWORD_LENGTH {
        return Err(StatusCode::BAD_REQUEST);
    }

    let estimate = zxcvbn(trimmed, user_inputs);
    if estimate.score() < MIN_PASSWORD_SCORE {
        return Err(StatusCode::BAD_REQUEST);
    }

    Ok(())
}

fn serialize_ticket_payload<T: Serialize>(payload: &T) -> Result<String, StatusCode> {
    serde_json::to_string(payload).map_err(|_| StatusCode::BAD_REQUEST)
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let db_url = std::env::var("DATABASE_URL").unwrap_or_else(|_| "sqlite:hstack_lite.db".to_string());
    
    let pool = SqlitePool::connect(&db_url).await?;
    
    // Minimal schema setup
    sqlx::query("CREATE TABLE IF NOT EXISTS users (id INTEGER PRIMARY KEY, first_name TEXT, email TEXT, password TEXT, created_at DATETIME)")
        .execute(&pool).await?;
    let _ = sqlx::query("ALTER TABLE users ADD COLUMN email TEXT")
        .execute(&pool)
        .await;
    sqlx::query("CREATE TABLE IF NOT EXISTS tickets (id TEXT PRIMARY KEY, userid INTEGER, type TEXT, payload TEXT, status TEXT, created_at DATETIME)")
        .execute(&pool).await?;

    let state = AppState { db: pool };

    let app = Router::new()
        .route("/api/auth/register", post(register))
        .route("/api/auth/login", post(login))
        .route("/api/tickets", get(get_tickets).post(create_ticket))
        .with_state(state);

    let addr = SocketAddr::from(([127, 0, 0, 1], 8001));
    println!("HStack Lite Server listening on {}", addr);
    let listener = tokio::net::TcpListener::bind(addr).await?;
    axum::serve(listener, app).await?;

    Ok(())
}

async fn register(State(state): State<AppState>, Json(payload): Json<UserCreate>) -> Result<Json<AuthResponse>, StatusCode> {
    let first_name = required_trimmed(&payload.first_name).ok_or(StatusCode::BAD_REQUEST)?;
    let email = required_trimmed(&payload.email).ok_or(StatusCode::BAD_REQUEST)?;
    let last_name = payload.last_name.unwrap_or_default().trim().to_string();
    validate_new_password(&payload.password, &[&first_name, &last_name, &email])?;

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

async fn get_tickets(
    State(state): State<AppState>,
    Query(query): Query<TicketQuery>,
) -> Result<Json<Vec<TicketDto>>, StatusCode> {
    let requested_user_id = query.userid.unwrap_or(1);
    let rows = sqlx::query(
        "SELECT id, userid, type, payload, status, created_at FROM tickets WHERE userid = ? ORDER BY created_at ASC",
    )
        .bind(requested_user_id)
        .fetch_all(&state.db)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    let mut tickets = Vec::new();
    for row in rows {
        let payload_str: String = row.get("payload");
        let payload = serde_json::from_str(&payload_str).map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
        tickets.push(TicketDto {
            id: row.get("id"),
            userid: row.get("userid"),
            r#type: row.get("type"),
            payload,
            status: row.get("status"),
            created_at: row.get::<String, _>("created_at"),
        });
    }
    
    Ok(Json(tickets))
}

async fn create_ticket(
    State(state): State<AppState>,
    Query(query): Query<TicketQuery>,
    Json(payload): Json<CreateTicketPayload>,
) -> Result<Json<serde_json::Value>, StatusCode> {
    let id = Uuid::new_v4().to_string();
    let requested_user_id = query.userid.unwrap_or(1);
    let payload_json = serialize_ticket_payload(&payload.payload)?;
    
    sqlx::query("INSERT INTO tickets (id, userid, type, payload, status, created_at) VALUES (?, ?, ?, ?, ?, ?)")
        .bind(&id)
        .bind(requested_user_id)
        .bind(&payload.r#type)
        .bind(&payload_json)
        .bind(&payload.status)
        .bind(Utc::now())
        .execute(&state.db)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    Ok(Json(serde_json::json!({ "id": id, "userid": requested_user_id, "status": "created" })))
}

#[cfg(test)]
mod tests {
    use super::{ensure_password_present, required_trimmed, validate_new_password};

    #[test]
    fn test_bcrypt_logic() {
        let password = "password123";
        let hashed = match bcrypt::hash(password, bcrypt::DEFAULT_COST) {
            Ok(value) => value,
            Err(error) => panic!("failed to hash password in test: {error}"),
        };

        let verified = match bcrypt::verify(password, &hashed) {
            Ok(value) => value,
            Err(error) => panic!("failed to verify password in test: {error}"),
        };

        assert!(verified);
    }

    #[test]
    fn rejects_blank_credentials() {
        assert_eq!(required_trimmed("  "), None);
        assert!(ensure_password_present(" ").is_err());
    }

    #[test]
    fn rejects_weak_new_passwords() {
        assert!(validate_new_password("short", &["antoine@example.com"]).is_err());
        assert!(validate_new_password("Antoine1234!", &["Antoine", "antoine@example.com"]).is_err());
        assert!(validate_new_password("correct horse battery staple 2049", &["antoine@example.com"]).is_ok());
    }
}
