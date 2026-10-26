---
name: doctor
description: Diagnose the selected Abbey backend and effective model using the local CLI.
disable-model-invocation: true
---

Run `abbey doctor` with the normal shell tool in the current workspace. Summarize the selected backend, effective model, readiness, and actionable failures from its actual output. Preserve a nonzero exit as a failure and distinguish unavailable evidence from success.

Do not change configuration, credentials, install software, download models, or start a daemon as part of diagnosis. Never print credential environment variables or bearer files. If the executable is missing, point to Abbey's installation instructions.
