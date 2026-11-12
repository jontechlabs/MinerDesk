//! Authenticated local facade; members forward plans to their pinned coordinator.
use super::*;
use crate::fleet::protocol::UpdateRolloutRequest;
pub(super) fn routes() -> Router<WebState> {
    Router::new()
        .route("/api/fleet/updates/rollouts", post(create))
        .route("/api/fleet/updates/rollouts/:id/cancel", post(cancel))
        .layer(axum::extract::DefaultBodyLimit::max(16 * 1024))
}
async fn create(
    AxumState(state): AxumState<WebState>,
    headers: HeaderMap,
    Json(request): Json<UpdateRolloutRequest>,
) -> Response {
    if let Err(error) = fleet_api::access(&headers, &state, false) {
        return error;
    }
    let manager = match fleet_api::manager(&state) {
        Ok(m) => m,
        Err(e) => return e,
    };
    fleet_api::response(manager.create_rollout(request).await)
}
async fn cancel(
    AxumState(state): AxumState<WebState>,
    headers: HeaderMap,
    AxumPath(id): AxumPath<String>,
) -> Response {
    if let Err(error) = fleet_api::access(&headers, &state, false) {
        return error;
    }
    let manager = match fleet_api::manager(&state) {
        Ok(m) => m,
        Err(e) => return e,
    };
    fleet_api::response(manager.cancel_rollout(&id).await)
}
