use axum::{
    Json,
    extract::{Path, Query, State},
    http::HeaderMap,
};

use crate::{
    auth::{issue_jwt, require_admin},
    db::{
        AppState, create_source, create_user, find_user_by_email,
        get_champion_data_by_source_and_alias, get_champion_data_by_source_and_id,
        list_active_sources, list_users, replace_champion_data, update_source, update_user,
        upsert_champion_data,
    },
    error::{AppError, AppResult},
    models::{
        BatchUpsertChampionDataRequest, BatchUpsertChampionDataResponse, ChampionDataResponse,
        CreateSourceRequest, CreateUserRequest, HealthResponse, LoginRequest, LoginResponse,
        ModeQuery, PublicSource, PublicUser, UpdateSourceRequest, UpdateUserRequest,
        UpsertChampionDataRequest, normalize_email, normalize_mode,
    },
};

pub async fn health() -> Json<HealthResponse> {
    Json(HealthResponse { status: "ok" })
}

pub async fn login(
    State(state): State<AppState>,
    Json(req): Json<LoginRequest>,
) -> AppResult<Json<LoginResponse>> {
    let email = normalize_email(&req.email);
    if email.is_empty() || req.password.is_empty() {
        return Err(AppError::BadRequest(
            "email and password are required".to_string(),
        ));
    }

    let Some(user) = find_user_by_email(&state.db, &email)
        .await
        .map_err(AppError::internal)?
    else {
        return Err(AppError::Unauthorized("invalid credentials".to_string()));
    };

    if user.is_active == 0 {
        return Err(AppError::Unauthorized("user is inactive".to_string()));
    }

    if !crate::auth::verify_password(&user.password_hash, &req.password)? {
        return Err(AppError::Unauthorized("invalid credentials".to_string()));
    }

    let token = issue_jwt(&user, &state.jwt_secret)?;
    let user = PublicUser::try_from(user).map_err(AppError::internal)?;
    Ok(Json(LoginResponse { token, user }))
}

pub async fn list_sources_handler(
    State(state): State<AppState>,
) -> AppResult<Json<Vec<PublicSource>>> {
    let sources = list_active_sources(&state.db).await?;
    Ok(Json(sources.into_iter().map(PublicSource::from).collect()))
}

pub async fn get_champion_by_id(
    State(state): State<AppState>,
    Path((source_key, champion_id)): Path<(String, i64)>,
    Query(query): Query<ModeQuery>,
) -> AppResult<Json<ChampionDataResponse>> {
    let data = get_champion_data_by_source_and_id(
        &state.db,
        &source_key,
        champion_id,
        &normalize_mode(query.mode.as_deref()),
    )
    .await?;

    Ok(Json(
        ChampionDataResponse::try_from_record(data).map_err(AppError::internal)?,
    ))
}

pub async fn get_champion_by_alias(
    State(state): State<AppState>,
    Path((source_key, champion_alias)): Path<(String, String)>,
    Query(query): Query<ModeQuery>,
) -> AppResult<Json<ChampionDataResponse>> {
    let data = get_champion_data_by_source_and_alias(
        &state.db,
        &source_key,
        &champion_alias,
        &normalize_mode(query.mode.as_deref()),
    )
    .await?;

    Ok(Json(
        ChampionDataResponse::try_from_record(data).map_err(AppError::internal)?,
    ))
}

pub async fn list_users_handler(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> AppResult<Json<Vec<PublicUser>>> {
    let _admin = require_admin(&headers, &state).await?;
    let users = list_users(&state.db).await?;

    let users = users
        .into_iter()
        .map(PublicUser::try_from)
        .collect::<Result<Vec<_>, _>>()
        .map_err(AppError::internal)?;

    Ok(Json(users))
}

pub async fn create_user_handler(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(req): Json<CreateUserRequest>,
) -> AppResult<Json<PublicUser>> {
    let _admin = require_admin(&headers, &state).await?;
    let user = create_user(&state.db, req).await?;
    Ok(Json(
        PublicUser::try_from(user).map_err(AppError::internal)?,
    ))
}

pub async fn update_user_handler(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<i64>,
    Json(req): Json<UpdateUserRequest>,
) -> AppResult<Json<PublicUser>> {
    let admin = require_admin(&headers, &state).await?;
    if admin.id == id && matches!(req.is_active, Some(false)) {
        return Err(AppError::BadRequest(
            "admin cannot deactivate the current account".to_string(),
        ));
    }

    let user = update_user(&state.db, id, req).await?;
    Ok(Json(
        PublicUser::try_from(user).map_err(AppError::internal)?,
    ))
}

pub async fn create_source_handler(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(req): Json<CreateSourceRequest>,
) -> AppResult<Json<PublicSource>> {
    let _admin = require_admin(&headers, &state).await?;
    let source = create_source(&state.db, req).await?;
    Ok(Json(PublicSource::from(source)))
}

pub async fn update_source_handler(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<i64>,
    Json(req): Json<UpdateSourceRequest>,
) -> AppResult<Json<PublicSource>> {
    let _admin = require_admin(&headers, &state).await?;
    let source = update_source(&state.db, id, req).await?;
    Ok(Json(PublicSource::from(source)))
}

pub async fn upsert_champion_data_handler(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(req): Json<UpsertChampionDataRequest>,
) -> AppResult<Json<ChampionDataResponse>> {
    let _admin = require_admin(&headers, &state).await?;
    let data = upsert_champion_data(&state.db, req).await?;
    Ok(Json(
        ChampionDataResponse::try_from_record(data).map_err(AppError::internal)?,
    ))
}

pub async fn batch_upsert_champion_data_handler(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(req): Json<BatchUpsertChampionDataRequest>,
) -> AppResult<Json<BatchUpsertChampionDataResponse>> {
    let _admin = require_admin(&headers, &state).await?;

    let mut items = Vec::with_capacity(req.items.len());
    for item in req.items {
        let row = upsert_champion_data(&state.db, item).await?;
        items.push(ChampionDataResponse::try_from_record(row).map_err(AppError::internal)?);
    }

    Ok(Json(BatchUpsertChampionDataResponse { items }))
}

pub async fn replace_champion_data_handler(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<i64>,
    Json(req): Json<UpsertChampionDataRequest>,
) -> AppResult<Json<ChampionDataResponse>> {
    let _admin = require_admin(&headers, &state).await?;
    let row = replace_champion_data(&state.db, id, req).await?;
    Ok(Json(
        ChampionDataResponse::try_from_record(row).map_err(AppError::internal)?,
    ))
}
