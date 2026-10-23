//! Exact signed-model lifecycle authority, separate from the read-only session.
use super::{Route, from_client_error, route};
use crate::ipc::{IpcError, IpcErrorKind};
use abbey::app_core::{
    V3Capability, V3CapabilitySet, V3ModelAction, V3ModelDetails, V3OperationStatus,
    V3ResourceQuery,
};
use abbey::daemon::V3DaemonSession;

const MODEL_GRANTS: &[V3Capability] = &[
    V3Capability::ReadModels,
    V3Capability::DownloadModels,
    V3Capability::ManageModels,
];
fn model_session() -> Result<V3DaemonSession, IpcError> {
    model_session_for(route()?)
}
fn model_session_for(route: Route) -> Result<V3DaemonSession, IpcError> {
    match route {
        Route::InProcess(_) => Err(IpcError::new(
            IpcErrorKind::Rejected,
            "model authority is daemon-only",
        )),
        Route::Daemon(client) => client
            .negotiate_v3(
                V3CapabilitySet::from_sorted(MODEL_GRANTS.to_vec())
                    .expect("canonical model grants"),
            )
            .map_err(from_client_error),
    }
}
pub fn model_grants() -> Result<V3CapabilitySet, IpcError> {
    let route = route()?;
    if matches!(route, Route::InProcess(_)) {
        return Ok(V3CapabilitySet::deny_all());
    }
    Ok(model_session_for(route)?.negotiation().granted.clone())
}
pub fn model_details(query: V3ResourceQuery) -> Result<V3ModelDetails, IpcError> {
    model_session()?
        .model_details(query)
        .map_err(from_client_error)
}
pub fn model_download(action: V3ModelAction) -> Result<V3OperationStatus, IpcError> {
    model_session()?
        .download_model(action)
        .map_err(from_client_error)
}
pub fn model_load(action: V3ModelAction) -> Result<V3OperationStatus, IpcError> {
    model_session()?
        .load_model(action)
        .map_err(from_client_error)
}
pub fn model_unload(action: V3ModelAction) -> Result<V3OperationStatus, IpcError> {
    model_session()?
        .unload_model(action)
        .map_err(from_client_error)
}
pub fn model_download_status(query: V3ResourceQuery) -> Result<V3OperationStatus, IpcError> {
    let status = model_session()?
        .model_download_status(query.clone())
        .map_err(from_client_error)?;
    exact_operation_status(&query, status)
}
pub fn model_lifecycle_status(query: V3ResourceQuery) -> Result<V3OperationStatus, IpcError> {
    // Existing ABI name includes inference; this bridge only queries operation IDs.
    let status = model_session()?
        .inference_status(query.clone())
        .map_err(from_client_error)?;
    exact_operation_status(&query, status)
}
fn exact_operation_status(
    query: &V3ResourceQuery,
    status: V3OperationStatus,
) -> Result<V3OperationStatus, IpcError> {
    if status.operation_id != query.resource_id {
        return Err(IpcError::new(
            IpcErrorKind::Protocol,
            "model operation identity mismatch",
        ));
    }
    Ok(status)
}
#[cfg(test)]
mod tests {
    use super::*;
    #[cfg(unix)]
    #[test]
    fn signed_model_lifecycle_bridge_fixture() {
        if std::env::var_os("ABBEY_DESKTOP_LIVE_MODELS").is_none() {
            return;
        }
        use abbey::app_core::{V3OperationState, V3PageQuery};
        let grants = model_grants().expect("live model grants");
        assert_eq!(grants.as_slice(), MODEL_GRANTS);
        let inventory =
            super::super::models_list(V3PageQuery::default()).expect("live model inventory");
        let id = std::env::var("ABBEY_DESKTOP_FIXTURE_MODEL").expect("fixture model ID");
        assert!(inventory.records.iter().any(|record| record.id == id));
        let query = V3ResourceQuery {
            resource_id: id.clone(),
        };
        let details = model_details(query.clone()).expect("exact signed fixture details");
        assert_eq!(details.model_id, id);
        assert!(details.license_accepted);
        assert!(!details.loaded);
        let action = |name: &str| V3ModelAction {
            model_id: id.clone(),
            revision: details.revision.clone(),
            operation_id: format!("desktop-live-{name}"),
        };
        fn terminal(action: &V3ModelAction, download: bool) -> V3OperationStatus {
            let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
            loop {
                let query = V3ResourceQuery {
                    resource_id: action.operation_id.clone(),
                };
                let status = if download {
                    model_download_status(query)
                } else {
                    model_lifecycle_status(query)
                }
                .expect("correlated live status");
                assert_eq!(status.operation_id, action.operation_id);
                assert_eq!(status.resource_id, action.model_id);
                if !matches!(
                    status.state,
                    V3OperationState::Queued | V3OperationState::Running
                ) {
                    return status;
                }
                assert!(
                    std::time::Instant::now() < deadline,
                    "fixture operation deadline"
                );
                std::thread::sleep(std::time::Duration::from_millis(20));
            }
        }
        let download = action("download");
        let submitted =
            model_download(download.clone()).expect("send-once fixture cached download");
        assert_eq!(submitted.operation_id, download.operation_id);
        assert_eq!(terminal(&download, true).state, V3OperationState::Succeeded);
        let load = action("load");
        model_load(load.clone()).expect("send-once fixture load");
        assert_eq!(terminal(&load, false).state, V3OperationState::Succeeded);
        assert!(model_details(query.clone()).unwrap().loaded);
        let unload = action("unload");
        model_unload(unload.clone()).expect("send-once fixture unload");
        assert_eq!(terminal(&unload, false).state, V3OperationState::Succeeded);
        assert!(!model_details(query).unwrap().loaded);
        // Unknown operation cannot be confused with a model's readiness state.
        assert!(
            model_lifecycle_status(V3ResourceQuery {
                resource_id: "desktop-live-missing".into()
            })
            .is_err()
        );
    }

    #[test]
    fn status_queries_only_accept_the_exact_operation_id() {
        let query = V3ResourceQuery {
            resource_id: "operation-1".into(),
        };
        let mut status = V3OperationStatus {
            operation_id: "other".into(),
            resource_id: "operation-1".into(),
            state: abbey::app_core::V3OperationState::Running,
            progress_basis_points: 0,
        };
        assert_eq!(
            exact_operation_status(&query, status.clone())
                .unwrap_err()
                .kind,
            IpcErrorKind::Protocol
        );
        status.operation_id = query.resource_id.clone();
        assert!(exact_operation_status(&query, status).is_ok());
    }
    #[test]
    fn model_grants_are_exact_and_separate() {
        let set = V3CapabilitySet::from_sorted(MODEL_GRANTS.to_vec()).unwrap();
        assert_eq!(
            set.as_slice(),
            &[
                V3Capability::ReadModels,
                V3Capability::DownloadModels,
                V3Capability::ManageModels
            ]
        );
        assert!(!set.contains(V3Capability::InferModels));
        assert!(!set.contains(V3Capability::InvokeTools));
    }
    #[test]
    fn missing_daemon_never_opens_model_authority() {
        if super::super::bearer_source().is_some() {
            return;
        }
        assert!(model_grants().unwrap().as_slice().is_empty());
        assert_eq!(
            model_details(V3ResourceQuery {
                resource_id: "fixture".into()
            })
            .unwrap_err()
            .kind,
            IpcErrorKind::Rejected
        );
        for effect in [model_download, model_load, model_unload] {
            assert_eq!(
                effect(V3ModelAction {
                    model_id: "fixture".into(),
                    revision: "v1".into(),
                    operation_id: "op-fixture".into()
                })
                .unwrap_err()
                .kind,
                IpcErrorKind::Rejected
            );
        }
        for status in [model_download_status, model_lifecycle_status] {
            assert_eq!(
                status(V3ResourceQuery {
                    resource_id: "op-fixture".into()
                })
                .unwrap_err()
                .kind,
                IpcErrorKind::Rejected
            );
        }
    }
}
