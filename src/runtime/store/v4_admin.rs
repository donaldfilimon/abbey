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
use rusqlite::{OptionalExtension, params};
const MAX_PROFILE_DOCUMENT_BYTES: usize = 65_536;

impl RuntimeStore {
    pub(crate) fn v4_put_permission_profile(
        &self,
        p: &V4PermissionProfile,
        now: u64,
    ) -> Result<(), StoreError> {
        p.validate()
            .map_err(|_| StoreError::InvalidInput("profile"))?;
        let document = serde_json::to_string(p)?;
        if document.len() > MAX_PROFILE_DOCUMENT_BYTES {
            return Err(StoreError::InvalidInput("profile document bound"));
        }
        let conn = self.conn.lock().expect("lock");
        conn.execute("INSERT INTO assistant_permission_profiles(profile_id, document_json, updated_at_ms) VALUES (?1,?2,?3) ON CONFLICT(profile_id) DO UPDATE SET document_json=excluded.document_json, updated_at_ms=excluded.updated_at_ms", params![&p.profile_id, document, super::ms(now)? ])?;
        Ok(())
    }
    pub(crate) fn v4_permission_profile(
        &self,
        id: &str,
    ) -> Result<V4PermissionProfile, StoreError> {
        let conn = self.conn.lock().expect("lock");
        // CASE bounds materialization in the same SQLite snapshot as lookup.
        let document: Option<Option<String>> = conn.query_row("SELECT CASE WHEN length(CAST(document_json AS BLOB)) BETWEEN 2 AND ?2 THEN document_json ELSE NULL END FROM assistant_permission_profiles WHERE profile_id=?1",params![id,MAX_PROFILE_DOCUMENT_BYTES as i64],|row|row.get(0)).optional()?;
        let document = document
            .ok_or(StoreError::AssistantNotFound)?
            .ok_or(StoreError::CorruptData("profile document bound"))?;
        let profile: V4PermissionProfile = serde_json::from_str(&document)
            .map_err(|_| StoreError::CorruptData("profile document invalid"))?;
        profile
            .validate()
            .map_err(|_| StoreError::CorruptData("profile invalid"))?;
        if profile.profile_id != id {
            return Err(StoreError::CorruptData("profile identity mismatch"));
        }
        Ok(profile)
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

#[cfg(test)]
#[path = "v4_profile_tests.rs"]
mod profile_tests;
