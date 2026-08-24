use super::{
    GuildIntelligenceError, GuildRecording, MAX_OBJECTS, MAX_REF_LEN, OperatorAuthority,
    OverwriteTarget, invalid,
};
use std::collections::BTreeSet;

impl GuildRecording {
    pub(super) fn validate(&self) -> Result<(), GuildIntelligenceError> {
        if self.schema_version != super::SCHEMA_VERSION {
            return invalid("unsupported schema version");
        }
        if matches!(self.operator_authority, OperatorAuthority::Owner)
            && self.operator_ref != self.owner_ref
        {
            return invalid("recorded owner authority does not match the recorded owner");
        }
        if self.roles.len() > MAX_OBJECTS
            || self.channels.len() > MAX_OBJECTS
            || self.active_threads.len() > MAX_OBJECTS
        {
            return invalid("object limit exceeded");
        }
        let refs = std::iter::once(&self.guild_ref)
            .chain(std::iter::once(&self.operator_ref))
            .chain(std::iter::once(&self.owner_ref))
            .chain(std::iter::once(&self.bot_self.ref_id))
            .chain(self.roles.iter().map(|item| &item.ref_id))
            .chain(self.channels.iter().map(|item| &item.ref_id))
            .chain(self.active_threads.iter().map(|item| &item.ref_id));
        if refs.into_iter().any(|value| {
            value.is_empty() || value.len() > MAX_REF_LEN || value.chars().any(char::is_control)
        }) {
            return invalid("invalid opaque reference");
        }
        let mut object_refs = BTreeSet::new();
        if self
            .roles
            .iter()
            .map(|item| item.ref_id.as_str())
            .chain(self.channels.iter().map(|item| item.ref_id.as_str()))
            .chain(self.active_threads.iter().map(|item| item.ref_id.as_str()))
            .any(|reference| !object_refs.insert(reference))
        {
            return invalid("duplicate metadata object reference");
        }
        let everyone_roles = self
            .roles
            .iter()
            .filter(|role| role.position == 0)
            .collect::<Vec<_>>();
        if everyone_roles.len() != 1 {
            return invalid("exactly one everyone role is required");
        }
        let everyone_ref = everyone_roles[0].ref_id.as_str();
        let role_refs: BTreeSet<_> = self.roles.iter().map(|role| role.ref_id.as_str()).collect();
        if self.bot_self.role_refs.len()
            != self
                .bot_self
                .role_refs
                .iter()
                .collect::<BTreeSet<_>>()
                .len()
        {
            return invalid("duplicate bot role reference");
        }
        if self
            .bot_self
            .role_refs
            .iter()
            .any(|reference| !role_refs.contains(reference.as_str()))
        {
            return invalid("bot role reference is missing");
        }
        let channel_refs: BTreeSet<_> = self
            .channels
            .iter()
            .map(|channel| channel.ref_id.as_str())
            .collect();
        for channel in &self.channels {
            if channel
                .parent_ref
                .as_deref()
                .is_some_and(|reference| !channel_refs.contains(reference))
            {
                return invalid("channel parent reference is missing");
            }
            let mut targets = BTreeSet::new();
            for overwrite in &channel.overwrites {
                let target_key = if overwrite.target.is_everyone(everyone_ref) {
                    (0, everyone_ref)
                } else {
                    overwrite.target.sort_key()
                };
                if !targets.insert(target_key) {
                    return invalid("duplicate channel overwrite target");
                }
                match &overwrite.target {
                    OverwriteTarget::Everyone { ref_id } if ref_id != everyone_ref => {
                        return invalid("everyone overwrite reference is invalid");
                    }
                    OverwriteTarget::Role { ref_id } if !role_refs.contains(ref_id.as_str()) => {
                        return invalid("overwrite role reference is missing");
                    }
                    OverwriteTarget::Member { ref_id } if ref_id != &self.bot_self.ref_id => {
                        return invalid("only the recorded bot member overwrite is accepted");
                    }
                    OverwriteTarget::Unrecognized { .. } => {
                        return invalid("unrecognized overwrite target");
                    }
                    _ => {}
                }
            }
        }
        if self
            .active_threads
            .iter()
            .any(|thread| !channel_refs.contains(thread.parent_ref.as_str()))
        {
            return invalid("active thread parent reference is missing");
        }
        if self
            .channels
            .iter()
            .map(|channel| channel.overwrites.len())
            .sum::<usize>()
            > MAX_OBJECTS
        {
            return invalid("overwrite limit exceeded");
        }
        if !(self.coverage.guild
            && self.coverage.roles
            && self.coverage.channels
            && self.coverage.active_threads)
        {
            return invalid("required recording surface is incomplete");
        }
        Ok(())
    }

    pub(super) fn normalize(&mut self) {
        self.roles.sort_by(|a, b| a.ref_id.cmp(&b.ref_id));
        self.channels.sort_by(|a, b| a.ref_id.cmp(&b.ref_id));
        self.active_threads.sort_by(|a, b| a.ref_id.cmp(&b.ref_id));
        self.bot_self.role_refs.sort();
        for channel in &mut self.channels {
            channel
                .overwrites
                .sort_by(|left, right| left.target.sort_key().cmp(&right.target.sort_key()));
        }
    }
}

impl OverwriteTarget {
    fn reference(&self) -> &str {
        match self {
            Self::Everyone { ref_id }
            | Self::Role { ref_id }
            | Self::Member { ref_id }
            | Self::Unrecognized { ref_id } => ref_id,
        }
    }

    pub(super) fn sort_key(&self) -> (u8, &str) {
        let rank = match self {
            Self::Everyone { .. } => 0,
            Self::Role { .. } => 1,
            Self::Member { .. } => 2,
            Self::Unrecognized { .. } => 3,
        };
        (rank, self.reference())
    }

    pub(super) fn is_everyone(&self, everyone_ref: &str) -> bool {
        matches!(self, Self::Everyone { ref_id } if ref_id == everyone_ref)
            || matches!(self, Self::Role { ref_id } if ref_id == everyone_ref)
    }
}
