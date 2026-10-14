use nanocode_code_mode::{attachment::ProcessRouteStore, contract::ToolError};
use nanocode_durability::{OwnedState, OwnerId, SharedStore, StateStore};
use std::{collections::HashMap, sync::Arc};
use tokio::sync::Mutex;

struct Routes {
    store: SharedStore,
    owners: HashMap<String, OwnedState>,
}

pub(crate) struct DurableProcessRoutes(Mutex<Routes>);

impl DurableProcessRoutes {
    pub(crate) fn new(store: SharedStore) -> Arc<Self> {
        Arc::new(Self(Mutex::new(Routes {
            store,
            owners: HashMap::new(),
        })))
    }
}

#[async_trait::async_trait]
impl ProcessRouteStore for DurableProcessRoutes {
    async fn load(&self, session_id: &str) -> Result<Option<serde_json::Value>, ToolError> {
        let mut routes = self.0.lock().await;
        let state_id = format!("{session_id}:process-routes");
        let owned = routes
            .store
            .acquire(&state_id, OwnerId::new())
            .await
            .map_err(|error| -> ToolError { Box::new(error) })?;
        let value = owned
            .state
            .payload
            .as_deref()
            .map(serde_json::from_str)
            .transpose()?;
        routes.owners.insert(state_id, owned);
        Ok(value)
    }

    async fn save(&self, session_id: &str, value: serde_json::Value) -> Result<(), ToolError> {
        let payload = serde_json::to_string(&value)?;
        let mut routes = self.0.lock().await;
        let state_id = format!("{session_id}:process-routes");
        let owned = routes
            .owners
            .get(&state_id)
            .cloned()
            .ok_or("process routes were not loaded before saving")?;
        let revision = routes
            .store
            .replace(&state_id, &owned.owner, owned.state.revision, &payload, &[])
            .await
            .map_err(|error| -> ToolError { Box::new(error) })?;
        routes.owners.insert(
            state_id,
            OwnedState {
                owner: owned.owner,
                state: nanocode_durability::StoredState {
                    revision,
                    payload: Some(payload),
                },
            },
        );
        Ok(())
    }
}
