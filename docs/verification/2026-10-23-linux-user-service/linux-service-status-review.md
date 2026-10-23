# Linux service status-contract scoped rereview

Original status-contract finding: ADDRESSED by static inspection. Spec compliance: PASS. Code quality: PASS; no fix-induced actionable findings.

The controller accepts precisely integer pairs (1,2) and (2,3), rejects Python boolean/int equivalence and crossed/unknown/string versions, and retains exact status-event, edition and ready-state checks. Current typed CLI protocol-2/schema-3 responses now pass this check; protocol-1/schema-2 compatibility remains accepted. No new transport fallback, authority, deadline or readiness-cleanup behavior is introduced. Extra current status fields remain owned and validated by the typed Rust client rather than a duplicate Python wire implementation.

Scoped inputs reviewed: linux-service-status.patch, linux-service-status-investigation.md, linux-service-actual-status.json and appended linux-service-report.md. Test additions cover both pairs in both editions and malformed/version/edition refusals. Controller-reported actual scratch-daemon success is separate evidence; it does not establish Ubuntu/systemd acceptance.

No builds, tests, agents, service operations or external actions performed. Only this requested external report was written. Static review does not independently execute the reported 21-test GREEN or actual-daemon proof.
