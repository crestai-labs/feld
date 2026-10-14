use std::sync::Arc;

use anyhow::Result;

use crate::store::{CreateWorkspace, Store, Workspace, WorkspaceDeleteError};

pub struct WorkspaceService {
    store: Arc<Store>,
}

impl WorkspaceService {
    pub fn new(store: Arc<Store>) -> Self {
        Self { store }
    }

    pub fn list(&self, include_deleted: bool) -> Result<Vec<Workspace>> {
        self.store.list_workspaces(include_deleted)
    }

    pub fn create(&self, key: &str) -> Result<CreateWorkspace> {
        self.store.create_workspace(key)
    }

    pub fn get(&self, key: &str) -> Result<Option<Workspace>> {
        self.store.get_workspace(key)
    }

    pub fn delete(&self, key: &str) -> std::result::Result<Workspace, WorkspaceDeleteError> {
        self.store.delete_workspace(key)
    }
}
