#![allow(
    dead_code,
    reason = "v4 admin stubs for Task2 conv focus; full in later"
)]
//! Permission profiles etc for v4 tests. Keep small.

use super::super::{RuntimeStore, StoreError};
#[allow(unused_imports)]
use crate::app_core::V4ResourceQuery; // prepared for future admin queries
use crate::app_core::{
    V4Automation, V4AutomationPage, V4AutomationUpsert, V4ConversationQuery, V4PermissionProfile,
    V4Task, V4TaskCreate, V4TaskPage,
};
use rusqlite::params;

impl RuntimeStore {
    pub(crate) fn v4_put_permission_profile(
        &self,
        p: &V4PermissionProfile,
        now: u64,
    ) -> Result<(), StoreError> {
        p.validate()
            .map_err(|_| StoreError::InvalidInput("profile"))?;
        let conn = self.conn.lock().expect("lock");
        conn.execute("INSERT OR REPLACE INTO assistant_permission_profiles(profile_id, document_json, updated_at_ms) VALUES (?1,?2,?3)", params![&p.profile_id, "{}", super::ms(now)? ])?;
        Ok(())
    }
    pub(crate) fn v4_permission_profile(
        &self,
        id: &str,
    ) -> Result<V4PermissionProfile, StoreError> {
        Ok(V4PermissionProfile {
            profile_id: id.to_owned(),
            tool_ids: vec![],
            application_ids: vec![],
            filesystem_roots: vec![],
            browser_domains: vec![],
            max_runtime_ms: 60000,
            max_output_bytes: 4096,
        })
    }
    pub(crate) fn v4_tasks(&self, _p: &V4ConversationQuery) -> Result<V4TaskPage, StoreError> {
        Ok(V4TaskPage {
            tasks: vec![],
            next_after: None,
        })
    }
    pub(crate) fn v4_create_task(&self, _t: &V4TaskCreate, _n: u64) -> Result<V4Task, StoreError> {
        Err(StoreError::InvalidInput("tasks later"))
    }
    pub(crate) fn v4_automations(
        &self,
        _p: &V4ConversationQuery,
    ) -> Result<V4AutomationPage, StoreError> {
        Ok(V4AutomationPage {
            automations: vec![],
            next_after: None,
        })
    }
    pub(crate) fn v4_upsert_automation(
        &self,
        _a: &V4AutomationUpsert,
        _n: u64,
    ) -> Result<V4Automation, StoreError> {
        Err(StoreError::InvalidInput("auto later"))
    }
    pub(crate) fn v4_decide_action(
        &self,
        _a: &str,
        _d: &str,
        _ap: bool,
        _n: u64,
    ) -> Result<(), StoreError> {
        Ok(())
    }
}

impl V4PermissionProfile {
    pub fn validate(&self) -> Result<(), crate::app_core::ValidationError> {
        Ok(())
    }
}
