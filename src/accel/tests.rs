//! Tests for the accelerator kernel-execution bridge.
//!
//! Without `--features accel` the only surface is the refusal, so that is the
//! only case compiled there. The parity cases branch on whether a native kernel
//! actually ran: on a host with no initialized Metal device the adapter returns
//! the CPU oracle, so an unconditional parity assertion would assert nothing.

/// Without the feature the verb must refuse loudly, never silently succeed.
#[cfg(not(feature = "accel"))]
#[test]
fn verify_without_the_feature_refuses_with_exit_two() {
    use super::{EXIT_UNAVAILABLE, verify_command};
    assert_eq!(EXIT_UNAVAILABLE, 2);
    assert_eq!(verify_command(&[]).unwrap(), EXIT_UNAVAILABLE);
    assert_eq!(
        verify_command(&["--json".to_string()]).unwrap(),
        EXIT_UNAVAILABLE
    );
}

#[cfg(feature = "accel")]
mod with_accel {
    // The bridge module is private to `accel`; a descendant test module may
    // still name it, and doing so keeps the parent's public surface narrow.
    use crate::accel::bridge::*;
    use crate::accel::verify_command;

    #[test]
    fn relative_tolerance_is_relative_with_an_absolute_floor() {
        // Near zero the floor makes the comparison absolute.
        assert!(relative_close(0.0, 0.000_5, PARITY_RELATIVE_TOLERANCE));
        assert!(!relative_close(0.0, 0.002, PARITY_RELATIVE_TOLERANCE));
        // At large magnitude it scales.
        assert!(relative_close(1000.0, 1000.5, PARITY_RELATIVE_TOLERANCE));
        assert!(!relative_close(1000.0, 1002.0, PARITY_RELATIVE_TOLERANCE));
    }

    #[test]
    fn fixtures_are_deterministic_and_exactly_representable() {
        let a = fixture_query(KERNEL_ELEMENTS);
        let b = fixture_operand(KERNEL_ELEMENTS);
        assert_eq!(a.len(), KERNEL_ELEMENTS);
        assert_eq!(b.len(), KERNEL_ELEMENTS);
        assert_eq!(a, fixture_query(KERNEL_ELEMENTS));
        assert_eq!(b, fixture_operand(KERNEL_ELEMENTS));
        // Every value is a multiple of 1/16, so no input rounding contributes
        // to a native/oracle divergence — only reduction order can.
        for v in a.iter().chain(&b) {
            assert!((v * 16.0).fract() == 0.0, "not a 1/16 multiple: {v}");
        }
    }

    #[test]
    fn top_k_fixture_candidates_are_distinct_directions() {
        let query = fixture_query(KERNEL_ELEMENTS);
        let operand = fixture_operand(KERNEL_ELEMENTS);
        let candidates: Vec<Vec<f32>> = (0..TOP_K_CANDIDATES)
            .map(|rank| fixture_candidate(&query, &operand, rank))
            .collect();
        for (i, candidate) in candidates.iter().enumerate() {
            assert_eq!(candidate.len(), KERNEL_ELEMENTS);
            for other in &candidates[i + 1..] {
                assert_ne!(candidate, other, "duplicate candidate at {i}");
            }
        }
        const { assert!(TOP_K_LIMIT < TOP_K_CANDIDATES) };
    }

    /// The oracle must rank the fixture unambiguously, or an index comparison
    /// would be testing tie-break order rather than the kernel.
    #[test]
    fn the_oracle_ranking_of_the_fixture_is_strictly_separated() {
        let cpu = abi_compute::CpuBackend::default();
        let query = fixture_query(KERNEL_ELEMENTS);
        let operand = fixture_operand(KERNEL_ELEMENTS);
        let candidates: Vec<Vec<f32>> = (0..TOP_K_CANDIDATES)
            .map(|rank| fixture_candidate(&query, &operand, rank))
            .collect();
        let refs: Vec<&[f32]> = candidates.iter().map(Vec::as_slice).collect();
        let ranked = cpu.top_k(&query, &refs, TOP_K_CANDIDATES).expect("top k");
        for pair in ranked.windows(2) {
            let gap = pair[0].score - pair[1].score;
            assert!(
                gap > PARITY_RELATIVE_TOLERANCE,
                "scores {} and {} are within tolerance — ranking is ambiguous",
                pair[0].score,
                pair[1].score
            );
        }
    }

    /// A CPU-only report must not describe itself as Metal-verified.
    #[test]
    fn a_report_with_no_native_execution_reports_cpu_and_is_not_verified() {
        let report = AccelReport::summarize(true, false, vec![check("dot", false, None)]);
        assert_eq!(report.backend_used, BackendUsed::Cpu);
        assert!(!report.verified);
        assert_eq!(report.exit_code(), EXIT_NOT_VERIFIED);
        let text = render(&report);
        assert!(text.contains("backend used:   cpu"), "{text}");
        assert!(text.contains("NOT VERIFIED"), "{text}");
        assert!(!text.contains("VERIFIED — every kernel"), "{text}");
    }

    /// Partial native execution must not be rounded up to "ran on Metal".
    #[test]
    fn partially_native_execution_reports_mixed_and_is_not_verified() {
        let report = AccelReport::summarize(
            true,
            true,
            vec![check("dot", true, Some(true)), check("top_k", false, None)],
        );
        assert_eq!(report.backend_used, BackendUsed::Mixed);
        assert!(!report.verified);
    }

    /// A native run that disagrees with the oracle is a mismatch, not a pass.
    #[test]
    fn a_native_mismatch_is_never_reported_as_verified() {
        let report = AccelReport::summarize(true, true, vec![check("dot", true, Some(false))]);
        assert_eq!(report.backend_used, BackendUsed::GpuMetal);
        assert!(!report.verified);
        assert_eq!(report.exit_code(), EXIT_NOT_VERIFIED);
        assert!(render(&report).contains("MISMATCH"));
    }

    #[test]
    fn a_fully_native_matching_run_is_verified_and_exits_zero() {
        let report = AccelReport::summarize(true, true, vec![check("dot", true, Some(true))]);
        assert_eq!(report.backend_used, BackendUsed::GpuMetal);
        assert!(report.verified);
        assert_eq!(report.exit_code(), 0);
    }

    /// The honesty boundary travels with every report.
    #[test]
    fn every_report_carries_the_not_proof_of_boundary() {
        let report = AccelReport::summarize(true, true, Vec::new());
        let text = render(&report);
        for claim in NOT_PROOF_OF {
            assert!(text.contains(claim), "render missing {claim}");
        }
        assert!(NOT_PROOF_OF.contains(&"device_residency_or_placement"));
        assert!(NOT_PROOF_OF.contains(&"speedup_or_performance"));
        assert!(NOT_PROOF_OF.contains(&"gpu_npu_tpu_compilation"));
        assert!(NOT_PROOF_OF.contains(&"training_or_model_inference"));
    }

    /// The real bridge: run the kernels and check the evidence ladder.
    #[test]
    fn kernels_match_the_cpu_oracle_when_they_execute_natively() {
        let report = verify().expect("verify");
        assert_eq!(report.kernels.len(), 3);
        assert_eq!(report.backend_requested, "gpu-metal");
        assert_eq!(report.relative_tolerance, PARITY_RELATIVE_TOLERANCE);

        for kernel in &report.kernels {
            if kernel.executed_natively {
                assert_eq!(
                    kernel.oracle_parity,
                    Some(true),
                    "{} executed on Metal but diverged from the CPU oracle: {}",
                    kernel.kernel,
                    kernel.detail
                );
            } else {
                assert_eq!(
                    kernel.oracle_parity, None,
                    "{} did not execute natively, so parity must not be recorded",
                    kernel.kernel
                );
            }
        }
    }

    /// The report must name the backend that ran, not the one that was wanted.
    #[test]
    fn the_report_names_the_backend_that_actually_ran() {
        let report = verify().expect("verify");
        let native = report
            .kernels
            .iter()
            .filter(|k| k.executed_natively)
            .count();

        match report.backend_used {
            BackendUsed::GpuMetal => {
                assert_eq!(native, report.kernels.len());
                // Metal cannot have run without a linked, initialized device.
                assert!(report.kernels_linked);
                assert!(report.device_initialized);
            }
            BackendUsed::Cpu => {
                assert_eq!(native, 0);
                assert!(!report.verified);
                assert!(render(&report).contains("NOT VERIFIED"));
            }
            BackendUsed::Mixed => {
                assert!(native > 0 && native < report.kernels.len());
                assert!(!report.verified);
            }
        }

        // `verified` may never outrun the device evidence.
        assert!(!report.verified || report.device_initialized);
        // Nor may the device claim outrun the link evidence.
        assert!(!report.device_initialized || report.kernels_linked);
    }

    #[test]
    fn verify_command_exit_code_tracks_the_report() {
        let expected = verify().expect("verify").exit_code();
        assert_eq!(verify_command(&[]).unwrap(), expected);
        assert_eq!(verify_command(&["--json".to_string()]).unwrap(), expected);
    }

    #[test]
    fn json_output_is_serializable_and_states_the_backend_used() {
        let report = verify().expect("verify");
        let json = serde_json::to_string(&report).expect("serialize");
        assert!(json.contains("\"backend_used\""), "{json}");
        assert!(
            json.contains(report.backend_used.label()),
            "{json} missing {}",
            report.backend_used.label()
        );
        assert!(json.contains("device_residency_or_placement"), "{json}");
    }

    fn check(kernel: &'static str, executed: bool, parity: Option<bool>) -> KernelCheck {
        KernelCheck {
            kernel,
            elements: KERNEL_ELEMENTS,
            executed_natively: executed,
            oracle_parity: parity,
            detail: "fixture".into(),
        }
    }
}
