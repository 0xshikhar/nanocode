use crate::{Tool, embedded::HostFuture};
use std::{
    ffi::OsString,
    path::{Path, PathBuf},
    sync::Arc,
};

pub trait WorkspaceExecutor: Send + Sync {
    fn tools(&self) -> Vec<Arc<dyn Tool>>;
    fn timeout_ms(&self, name: &str) -> Option<u64>;
    fn cancel<'a>(&'a self) -> HostFuture<'a, ()>;
}

pub trait WorkspaceToolFactory: Send + Sync {
    fn create(
        &self,
        root: PathBuf,
        environment: Vec<(OsString, OsString)>,
        max_wire_bytes: Option<u64>,
    ) -> Arc<dyn WorkspaceExecutor>;

    fn code_mode_policy(&self) -> Option<Arc<dyn crate::CodeModeToolPolicy>> {
        None
    }
}

#[derive(Clone)]
pub struct WorkspaceTools {
    root: PathBuf,
    factory: Arc<dyn WorkspaceToolFactory>,
}

impl WorkspaceTools {
    pub fn with_factory(
        root: impl Into<PathBuf>,
        factory: impl WorkspaceToolFactory + 'static,
    ) -> Self {
        Self {
            root: root.into(),
            factory: Arc::new(factory),
        }
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    pub fn create(
        &self,
        environment: Vec<(OsString, OsString)>,
        max_wire_bytes: Option<u64>,
    ) -> Arc<dyn WorkspaceExecutor> {
        self.factory
            .create(self.root.clone(), environment, max_wire_bytes)
    }

    #[must_use]
    pub fn code_mode_policy(&self) -> Option<Arc<dyn crate::CodeModeToolPolicy>> {
        self.factory.code_mode_policy()
    }
}
