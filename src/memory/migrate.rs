//! Move memory records between backends.

use super::MemoryStore;

/// What a migration actually did.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct MigrationReport {
    /// Records written to the destination.
    pub migrated: usize,
    /// Records read back from the destination and compared field-for-field.
    /// Equal to `migrated` on success; the migration errors rather than
    /// reporting a gap, because a partial copy the operator believes is whole
    /// is worse than a loud failure.
    pub verified: usize,
}

/// Report what [`migrate`] would do, writing nothing.
///
/// Runs the same emptiness check as the real thing, so a refusal surfaces
/// before an operator commits to changing where their memories live.
pub fn plan(src: &dyn MemoryStore, dst: &dyn MemoryStore) -> anyhow::Result<MigrationReport> {
    refuse_non_empty(dst)?;
    Ok(MigrationReport {
        migrated: src.all_records_including_obsolete()?.len(),
        verified: 0,
    })
}

fn refuse_non_empty(dst: &dyn MemoryStore) -> anyhow::Result<()> {
    let existing = dst.all_records_including_obsolete()?;
    if !existing.is_empty() {
        anyhow::bail!(
            "destination is not empty: {} record(s) already present. \
             Both backends insert rather than upsert, so migrating into it would \
             either fail part-way or merge two histories.",
            existing.len()
        );
    }
    Ok(())
}

/// Copy every record from `src` into `dst`, preserving record identity.
pub fn migrate(src: &dyn MemoryStore, dst: &dyn MemoryStore) -> anyhow::Result<MigrationReport> {
    refuse_non_empty(dst)?;
    let records = src.all_records_including_obsolete()?;
    let mut migrated = 0;
    let mut verified = 0;
    for record in records {
        let expected = record.clone();
        dst.store(record)?;
        migrated += 1;

        let Some(actual) = dst.get(&expected.id)? else {
            anyhow::bail!(
                "wrote memory {} but could not read it back; destination is now a partial copy",
                expected.id
            );
        };
        if actual.id != expected.id
            || actual.timestamp != expected.timestamp
            || actual.provenance != expected.provenance
            || actual.summary != expected.summary
            || actual.payload != expected.payload
            || actual.retention != expected.retention
            || actual.obsolete != expected.obsolete
        {
            anyhow::bail!(
                "memory {} did not survive the crossing unchanged; destination is now a partial copy",
                expected.id
            );
        }
        verified += 1;
    }
    Ok(MigrationReport { migrated, verified })
}

#[cfg(test)]
#[path = "migrate_tests.rs"]
mod tests;
