mod auth;
mod config;
mod db;
mod error;
mod handlers;
mod models;

use axum::{
    Router,
    routing::{get, patch, post, put},
};
pub use config::Config;
pub use db::AppState;
use handlers::{
    batch_upsert_champion_data_handler, create_source_handler, create_user_handler,
    get_champion_by_alias, get_champion_by_id, health, list_sources_handler, list_users_handler,
    login, replace_champion_data_handler, update_source_handler, update_user_handler,
    upsert_champion_data_handler,
};
use tower_http::trace::TraceLayer;

pub async fn run(config: Config) -> anyhow::Result<()> {
    let state = db::init_state(config).await?;
    let app = build_router(state.clone());
    let listener = tokio::net::TcpListener::bind(&state.addr).await?;
    let local_url = local_access_url(&state.addr);

    tracing::info!(address = %state.addr, local_url = %local_url, "server listening");
    axum::serve(listener, app).await?;
    Ok(())
}

fn local_access_url(addr: &str) -> String {
    if let Some(port) = addr.strip_prefix("0.0.0.0:") {
        return format!("http://127.0.0.1:{port}");
    }

    if let Some(port) = addr.strip_prefix("[::]:") {
        return format!("http://127.0.0.1:{port}");
    }

    format!("http://{addr}")
}

pub fn build_router(state: AppState) -> Router {
    Router::new()
        .route("/health", get(health))
        .route("/api/auth/login", post(login))
        .route("/api/sources", get(list_sources_handler))
        .route(
            "/api/source/{source}/champion-id/{champion_id}",
            get(get_champion_by_id),
        )
        .route(
            "/api/source/{source}/champion-alias/{champion_alias}",
            get(get_champion_by_alias),
        )
        .route(
            "/api/admin/users",
            get(list_users_handler).post(create_user_handler),
        )
        .route("/api/admin/users/{id}", patch(update_user_handler))
        .route("/api/admin/sources", post(create_source_handler))
        .route("/api/admin/sources/{id}", patch(update_source_handler))
        .route(
            "/api/admin/champion-data",
            post(upsert_champion_data_handler),
        )
        .route(
            "/api/admin/champion-data/batch",
            post(batch_upsert_champion_data_handler),
        )
        .route(
            "/api/admin/champion-data/{id}",
            put(replace_champion_data_handler),
        )
        .layer(TraceLayer::new_for_http())
        .with_state(state)
}

#[cfg(test)]
mod tests {
    use axum::{
        body::Body,
        http::{Request, StatusCode, header},
    };
    use base64::{Engine as _, engine::general_purpose::STANDARD};
    use serde_json::{Value, json};
    use tower::util::ServiceExt;

    use crate::{
        Config, build_router,
        db::{create_user, init_state},
        models::{CreateUserRequest, UserRole},
    };

    async fn test_app() -> (crate::AppState, axum::Router) {
        let temp = tempfile::tempdir().unwrap();
        let db_path = temp.path().join("test.db");
        let config = Config {
            addr: "127.0.0.1:0".to_string(),
            database_url: format!("sqlite://{}", db_path.to_string_lossy()),
            jwt_secret: "test-secret".to_string(),
            bootstrap_admin_email: None,
            bootstrap_admin_password: None,
        };

        let state = init_state(config).await.unwrap();
        let _hold = Box::leak(Box::new(temp));
        let router = build_router(state.clone());
        (state, router)
    }

    async fn seed_users(state: &crate::AppState) {
        create_user(
            &state.db,
            CreateUserRequest {
                email: "admin@example.com".to_string(),
                password: "password123".to_string(),
                role: UserRole::Admin,
                is_active: Some(true),
            },
        )
        .await
        .unwrap();

        create_user(
            &state.db,
            CreateUserRequest {
                email: "user@example.com".to_string(),
                password: "password123".to_string(),
                role: UserRole::User,
                is_active: Some(true),
            },
        )
        .await
        .unwrap();
    }

    async fn login(router: &axum::Router, email: &str, password: &str) -> String {
        let response = router
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/auth/login")
                    .header(header::CONTENT_TYPE, "application/json")
                    .body(Body::from(
                        json!({ "email": email, "password": password }).to_string(),
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);
        let body = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();
        let payload: Value = serde_json::from_slice(&body).unwrap();
        payload["token"].as_str().unwrap().to_string()
    }

    fn basic_auth(email: &str, password: &str) -> String {
        let raw = format!("{email}:{password}");
        format!("Basic {}", STANDARD.encode(raw))
    }

    #[tokio::test]
    async fn admin_can_upsert_and_read_latest_champion_data() {
        let (state, router) = test_app().await;
        seed_users(&state).await;
        let token = login(&router, "admin@example.com", "password123").await;

        let first = router
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/admin/champion-data")
                    .header(header::AUTHORIZATION, format!("Bearer {token}"))
                    .header(header::CONTENT_TYPE, "application/json")
                    .body(Body::from(
                        json!({
                            "source_key": "op.gg",
                            "champion_id": 107,
                            "champion_alias": "rengar",
                            "version": "16.6.1",
                            "content": [{ "build": "first" }]
                        })
                        .to_string(),
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(first.status(), StatusCode::OK);

        let second = router
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/admin/champion-data")
                    .header(header::AUTHORIZATION, format!("Bearer {token}"))
                    .header(header::CONTENT_TYPE, "application/json")
                    .body(Body::from(
                        json!({
                            "source_key": "op.gg",
                            "champion_id": 107,
                            "champion_alias": "Rengar",
                            "version": "16.6.2",
                            "content": [{ "build": "latest" }]
                        })
                        .to_string(),
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(second.status(), StatusCode::OK);

        let by_id = router
            .clone()
            .oneshot(
                Request::builder()
                    .method("GET")
                    .uri("/api/source/op.gg/champion-id/107")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(by_id.status(), StatusCode::OK);
        let body = axum::body::to_bytes(by_id.into_body(), usize::MAX)
            .await
            .unwrap();
        let payload: Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(payload["version"], "16.6.2");
        assert_eq!(payload["champion_alias"], "rengar");
        assert_eq!(payload["content"][0]["build"], "latest");

        let by_alias = router
            .clone()
            .oneshot(
                Request::builder()
                    .method("GET")
                    .uri("/api/source/op.gg/champion-alias/Rengar")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(by_alias.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn normal_user_is_forbidden_from_admin_routes() {
        let (state, router) = test_app().await;
        seed_users(&state).await;

        let response = router
            .clone()
            .oneshot(
                Request::builder()
                    .method("GET")
                    .uri("/api/admin/users")
                    .header(
                        header::AUTHORIZATION,
                        basic_auth("user@example.com", "password123"),
                    )
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::FORBIDDEN);
    }

    #[tokio::test]
    async fn basic_auth_works_for_admin_requests() {
        let (state, router) = test_app().await;
        seed_users(&state).await;

        let response = router
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/admin/sources")
                    .header(
                        header::AUTHORIZATION,
                        basic_auth("admin@example.com", "password123"),
                    )
                    .header(header::CONTENT_TYPE, "application/json")
                    .body(Body::from(
                        json!({
                            "key": "u.gg",
                            "label": "U.GG"
                        })
                        .to_string(),
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);

        let sources = router
            .clone()
            .oneshot(
                Request::builder()
                    .method("GET")
                    .uri("/api/sources")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(sources.status(), StatusCode::OK);
        let body = axum::body::to_bytes(sources.into_body(), usize::MAX)
            .await
            .unwrap();
        let payload: Value = serde_json::from_slice(&body).unwrap();
        assert!(
            payload
                .as_array()
                .unwrap()
                .iter()
                .any(|item| item["value"] == "u.gg")
        );
    }
}
