//! Read-only protocol-v3 tool inventory.
//!
//! One grant, and only one: [`V3Capability::ListTools`]. The desktop never
//! requests `InvokeTools`, `DecideToolApprovals`, or `CancelTools`, so this
//! session *cannot* run a tool, answer an approval, or cancel one. Listing a
//! tool is not calling it, and this module keeps that distinction structural
//! rather than a matter of frontend discipline.
//!
//! The inventory is daemon-only: the in-process application core is not a tool
//! host, so with no bearer configured this read is refused rather than
//! answered from this process. A daemon that does not negotiate `ListTools`
//! refuses the same way.

use abbey::app_core::{V3Capability, V3CapabilitySet, V3PageQuery, V3ToolPage};
use abbey::daemon::V3DaemonSession;

use super::{Route, bearer_source, from_client_error, route};
use crate::ipc::{IpcError, IpcErrorKind};

/// The exact grant set this session negotiates. Read-only by construction.
const TOOL_GRANTS: &[V3Capability] = &[V3Capability::ListTools];

fn tool_session() -> Result<V3DaemonSession, IpcError> {
    if bearer_source().is_none() {
        return Err(IpcError::new(
            IpcErrorKind::Rejected,
            "the tool inventory requires a configured abbeyd",
        ));
    }
    match route()? {
        Route::InProcess(_) => Err(IpcError::new(
            IpcErrorKind::Rejected,
            "the tool inventory is daemon-only and never opens an in-process store",
        )),
        Route::Daemon(client) => client
            .negotiate_v3(
                V3CapabilitySet::from_sorted(TOOL_GRANTS.to_vec()).expect("canonical tool grants"),
            )
            .map_err(from_client_error),
    }
}

/// `ListTools` — one bounded page of the daemon's advertised tools.
///
/// The descriptors carry no invocation authority: this session holds no
/// `InvokeTools` grant, so nothing here can be run from the desktop.
pub fn tools_list(query: V3PageQuery) -> Result<V3ToolPage, IpcError> {
    tool_session()?.list_tools(query).map_err(from_client_error)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ipc::IpcErrorKind;

    #[test]
    fn the_tool_session_requests_enumeration_and_nothing_else() {
        let requested =
            V3CapabilitySet::from_sorted(TOOL_GRANTS.to_vec()).expect("canonical tool grants");
        assert_eq!(requested.as_slice(), &[V3Capability::ListTools]);
        // The grants that would turn an inventory into an action surface. If
        // one of these is ever added to `TOOL_GRANTS`, this test fails rather
        // than handing the webview an invocation or approval authority.
        for forbidden in [
            V3Capability::InvokeTools,
            V3Capability::DecideToolApprovals,
            V3Capability::CancelTools,
            V3Capability::ReadMemory,
            V3Capability::ReadModels,
            V3Capability::DownloadModels,
            V3Capability::ManageModels,
            V3Capability::ReadTraining,
            V3Capability::ManageTraining,
            V3Capability::ReadWorkers,
            V3Capability::CancelJobs,
            V3Capability::ReadClaimsById,
            V3Capability::PollEvents,
            V3Capability::InferModels,
        ] {
            assert!(
                !requested.contains(forbidden),
                "the desktop tool session must never request {forbidden:?}"
            );
        }
    }

    #[test]
    fn without_a_daemon_the_tool_inventory_is_refused() {
        if crate::backend::bearer_source().is_some() {
            return; // covered against a real daemon by the live proof below
        }
        let error =
            tools_list(V3PageQuery::default()).expect_err("the in-process core is not a tool host");
        assert_eq!(error.kind, IpcErrorKind::Rejected);
        assert!(
            !error.message.contains("token") && !error.message.contains("bearer"),
            "the refusal must not mention credential material: {error:?}"
        );
    }

    /// Live proof against a real `abbeyd`: the inventory arrives, it is
    /// bounded, and every descriptor the daemon advertises is one the desktop
    /// may only *list*.
    ///
    /// Vacuous unless `ABBEY_DESKTOP_LIVE_DAEMON=1` is exported by
    /// `desktop/scripts/prove-daemon-read.sh` before this process exists
    /// (`set_var` is denied in this crate).
    #[cfg(unix)]
    #[test]
    fn live_daemon_desktop_lists_the_tool_inventory() {
        if std::env::var_os("ABBEY_DESKTOP_LIVE_DAEMON").is_none() {
            return;
        }
        let page = tools_list(V3PageQuery::default()).expect("desktop tool inventory");
        page.validate().expect("the daemon page must be valid");
        assert!(!page.tools.is_empty(), "the daemon advertises tools");
        for tool in &page.tools {
            // The desktop's whole claim is that it can show a tool and not
            // run it. Every advertised effect is rendered as a class; none of
            // them is an invitation.
            assert!(!tool.tool_id.is_empty());
            assert!(!tool.description.is_empty());
            assert!(
                tool.input_schema.is_object(),
                "a tool descriptor carries an object schema"
            );
        }
        // The safe daemon also advertises the one mutating tool. It is listed
        // with its effect class and nothing else: this session holds no
        // `InvokeTools` grant, so it cannot call it.
        let mutating = page
            .tools
            .iter()
            .filter(|tool| tool.effect == abbey::app_core::V3ToolEffect::Mutating);
        for tool in mutating {
            assert_eq!(tool.tool_id, "abbey_memory_mark_obsolete");
        }
    }
}
