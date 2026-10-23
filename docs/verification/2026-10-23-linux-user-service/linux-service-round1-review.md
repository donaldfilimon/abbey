# Linux service round 1 scoped rereview

Original finding: ADDRESSED by static inspection. Post-publication validation/reload failure now restores validated prior unit bytes and permission mode, or removes a newly created unit. Credentials and runtime state remain retained. A rollback reload is attempted; failed file rollback, failed manager reload after restoration, and successful restoration/reload produce distinct failure diagnostics. Installation never claims success after the original failure.

Spec compliance: PASS for this narrowly reviewed correction. Code quality: PASS by static inspection; no fix-induced actionable findings. The atomic staging helper preserves prior content when publication itself fails, cleans staging files, and restores mode before publication on rollback. Added regressions cover both first-install and previous-unit cases, retained state, and the three rollback diagnostic outcomes. Documentation now matches the required failure contract.

Reviewed only linux-service-round1.patch and appended linux-service-report.md against the original finding. No builds, tests, agents, service operations or unrelated review performed; only this requested external report was written. Reported Python RED/GREEN was not independently executed. Native systemd, real-daemon and broader gate qualification remain outside this static verdict.
