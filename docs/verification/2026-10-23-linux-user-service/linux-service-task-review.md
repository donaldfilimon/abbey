# Linux user-service slice review

Spec compliance: NEEDS CHANGE. One failure-atomicity requirement from linux-service-brief.md remains unmet. Native systemd and real-daemon acceptance remain correctly unclaimed.

Code quality: NEEDS CHANGE for the installation failure contract below; otherwise no discrete introduced defect found by static inspection. Existing CLI identity flags, daemon no-argument startup, authenticated status JSON shape/schema/edition, and edition environment names match current Rust contracts. No shell, inline bearer, provider authority, automatic enablement, or user-data deletion was added.

## Actionable finding

[P2] tools/linux_user_service.py:167–171 publishes the unit before daemon-reload, but leaves it published when daemon-reload fails. On a first install, the command reports failure while a new executable service definition remains installed. This conflicts with the brief's explicit “Avoid partial replacement” and “preserve previous content on failure” contract. Documentation disclosing this behavior does not implement that requirement. On reload failure, restore the exact previous unit or remove only the newly created unit, preserving retained credentials/state; report any rollback/reload failure explicitly. Add a fake-systemctl daemon-reload failure regression that verifies unit restoration/absence and credential retention. Existing exact-content validation means replacement cannot adopt a different old configuration, which limits the problem but does not eliminate first-install partial publication.

## Controller concerns and limits

The runner bounds its own captured read and wait, including descendant-held stdout, but only kills/reaps the direct process. Surviving descendants are a real limitation, explicitly disclosed in the implementation report. This brief does not explicitly require general process-tree supervision for this controller's trusted installed executables, so I do not classify that alone as a blocking spec failure. It must not be described as descendant teardown or full bounded process-tree proof. General Completion Program teardown requirements remain open.

Retention and isolation: uninstall validates exact owned unit content, stops/disables that edition only, removes its unit, and retains credentials and runtime state. Failed readiness requests stop; a stop failure itself returns failure. No ready claim is emitted on those failures. Conservative permissions and relocation restrictions are documented. Fixture results cannot qualify native Linux systemd semantics or real authenticated daemon startup.

Reviewed linux-service-brief.md, linux-service-report.md, linux-service-review.patch, all three canonical new files, root AGENTS.md applicable boundaries, and relevant Rust status/edition/environment contracts. Commands were read-only cat, sed, rg and nl; only this requested external review artifact was written. No builds, tests, service operations, agents, external actions, or unrelated source modifications were performed.
