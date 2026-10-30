//! Read-only prerequisites shared by execution and conservative ordering probes.

use anyhow::Result;

use super::{Context, TaskResult};

/// The stage whose prerequisites are being inspected.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RequirementStage {
    /// Prove an ordering-only wait can be waived before dispatch.
    Ordering,
    /// Inspect the inputs needed to preview work, not to apply it.
    Preview,
    /// Inspect the inputs needed to execute work after dependencies finish.
    Apply,
}

/// Observed readiness, including the evidence needed by the consumer.
#[derive(Debug)]
pub enum RequirementState<T> {
    /// The condition holds for this stage.
    Satisfied(T),
    /// Inspection succeeded, but a required input is unavailable.
    Unsatisfied {
        /// Actionable explanation of the unmet condition.
        reason: String,
    },
    /// Inspection failed; the condition is neither proven nor disproven.
    Unknown {
        /// Original error, retaining typed causes such as cancellation.
        error: anyhow::Error,
    },
}

impl<T> RequirementState<T> {
    /// Describe an observed unmet prerequisite.
    #[must_use]
    pub fn unsatisfied(reason: impl Into<String>) -> Self {
        Self::Unsatisfied {
            reason: reason.into(),
        }
    }

    fn discard_evidence(self) -> RequirementState<()> {
        match self {
            Self::Satisfied(_) => RequirementState::Satisfied(()),
            Self::Unsatisfied { reason } => RequirementState::Unsatisfied { reason },
            Self::Unknown { error } => RequirementState::Unknown { error },
        }
    }
}

type Probe<'a, T> =
    dyn Fn(&Context, RequirementStage) -> Result<RequirementState<T>> + Send + Sync + 'a;

/// A named, read-only condition, not an instruction to install its prerequisites.
///
/// Probes must not mutate managed state. Evidence is local to one check:
/// ordering assessments never supply cached inputs to later execution.
pub struct Requirement<'a, T = ()> {
    id: &'static str,
    probe: Box<Probe<'a, T>>,
}

impl<T> std::fmt::Debug for Requirement<'_, T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Requirement").field("id", &self.id).finish()
    }
}

impl<'a, T: 'a> Requirement<'a, T> {
    /// Declare a probe. Errors become unknown readiness without losing their cause.
    pub fn new(
        id: &'static str,
        probe: impl Fn(&Context, RequirementStage) -> Result<RequirementState<T>> + Send + Sync + 'a,
    ) -> Self {
        Self {
            id,
            probe: Box::new(probe),
        }
    }

    /// Stable diagnostic identity, independent of a task's display name.
    #[must_use]
    pub const fn id(&self) -> &'static str {
        self.id
    }

    /// Inspect once for the requested stage without retaining the result.
    pub fn check(&self, ctx: &Context, stage: RequirementStage) -> RequirementState<T> {
        let readiness = match (self.probe)(ctx, stage) {
            Ok(readiness) => readiness,
            Err(error) => RequirementState::Unknown { error },
        };
        match readiness {
            RequirementState::Satisfied(evidence) => RequirementState::Satisfied(evidence),
            RequirementState::Unsatisfied { reason } => {
                ctx.debug_fmt(|| format!("requirement {} ({stage:?}): {reason}", self.id()));
                RequirementState::Unsatisfied { reason }
            }
            RequirementState::Unknown { error } => {
                let error = error.context(format!("checking requirement {}", self.id()));
                ctx.debug_fmt(|| {
                    format!("requirement {} ({stage:?}) unknown: {error:#}", self.id())
                });
                RequirementState::Unknown { error }
            }
        }
    }

    /// Use the same typed probe where ordering needs only proof, not its evidence.
    #[must_use]
    pub fn without_evidence(self) -> Requirement<'a> {
        Requirement::new(self.id, move |ctx, stage| {
            (self.probe)(ctx, stage).map(RequirementState::discard_evidence)
        })
    }
}

impl Requirement<'static> {
    /// Require an executable according to the injected executor's PATH lookup.
    #[must_use]
    pub fn command(id: &'static str, program: &'static str, missing: &'static str) -> Self {
        Self::new(id, move |ctx, _| {
            Ok(if ctx.which(program) {
                RequirementState::Satisfied(())
            } else {
                RequirementState::unsatisfied(missing)
            })
        })
    }
}

/// Check an execution prerequisite and consume its exact evidence once.
///
/// Call from the task body, after dependencies finish. Preview checks only the
/// preview-stage contract. The continuation still owns preview versus mutation,
/// normally through the resource or operation lifecycle.
///
/// # Errors
///
/// Returns the original inspection error or a failure from the continuation.
pub fn process_requirement<T>(
    ctx: &Context,
    requirement: &Requirement<'_, T>,
    run: impl FnOnce(T) -> Result<TaskResult>,
) -> Result<TaskResult> {
    let stage = if ctx.dry_run() {
        RequirementStage::Preview
    } else {
        RequirementStage::Apply
    };
    match requirement.check(ctx, stage) {
        RequirementState::Satisfied(evidence) => run(evidence),
        RequirementState::Unsatisfied { reason } => Ok(TaskResult::unmet(reason)),
        RequirementState::Unknown { error } => Err(error),
    }
}

/// Empty declarations are not proof that a task can safely skip its waits.
pub(crate) fn ordering_requirements_satisfied(
    ctx: &Context,
    requirements: &[Requirement<'_>],
) -> bool {
    !requirements.is_empty()
        && requirements.iter().all(|requirement| {
            matches!(
                requirement.check(ctx, RequirementStage::Ordering),
                RequirementState::Satisfied(())
            )
        })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};

    use crate::infra::exec::{ExecError, ExecResult};
    use crate::test_helpers::make_static_context;

    #[test]
    fn execution_checks_once_and_consumes_exact_evidence_for_its_stage() {
        let (ctx, _) = make_static_context("/tmp".into(), None);
        for dry_run in [false, true] {
            let calls = AtomicUsize::new(0);
            let requirement = Requirement::new("resolved-launcher", |_, stage| {
                assert_eq!(
                    stage,
                    if dry_run {
                        RequirementStage::Preview
                    } else {
                        RequirementStage::Apply
                    },
                );
                assert_eq!(calls.fetch_add(1, Ordering::SeqCst), 0);
                Ok(RequirementState::Satisfied(String::from(
                    "/fixture/launcher",
                )))
            });
            assert_eq!(
                format!("{requirement:?}"),
                "Requirement { id: \"resolved-launcher\" }",
                "diagnostic formatting identifies the requirement without exposing its probe"
            );
            let result = process_requirement(&ctx.with_dry_run(dry_run), &requirement, |path| {
                assert_eq!(path, "/fixture/launcher");
                Ok(TaskResult::CheckPassed)
            })
            .unwrap();
            assert!(matches!(result, TaskResult::CheckPassed));
            assert_eq!(calls.load(Ordering::SeqCst), 1);
        }
    }

    #[test]
    fn unmet_and_unknown_requirements_never_run_the_consumer() {
        let (ctx, _) = make_static_context("/tmp".into(), None);
        let log = std::sync::Arc::new(crate::test_helpers::CapturingOutput::default());
        let ctx = ctx.with_log(std::sync::Arc::<crate::test_helpers::CapturingOutput>::clone(&log));
        for dry_run in [false, true] {
            for unknown in [false, true] {
                let requirement = Requirement::<()>::new("fixture-input", |_, _| {
                    if unknown {
                        Err(std::io::Error::new(
                            std::io::ErrorKind::PermissionDenied,
                            "probe refused",
                        )
                        .into())
                    } else {
                        Ok(RequirementState::unsatisfied("fixture input is missing"))
                    }
                });
                let result = process_requirement(&ctx.with_dry_run(dry_run), &requirement, |()| {
                    panic!("an unproven requirement must never execute work");
                });
                if unknown {
                    let error = result.unwrap_err();
                    assert_eq!(
                        error.downcast_ref::<std::io::Error>().unwrap().kind(),
                        std::io::ErrorKind::PermissionDenied
                    );
                    assert!(error.to_string().contains("fixture-input"));
                } else {
                    assert!(
                        matches!(result.unwrap(), TaskResult::Skipped { reason, kind: super::super::SkipKind::UnmetWork } if reason == "fixture input is missing")
                    );
                }
            }
        }
        assert!(log.messages().iter().any(|(_, message)| {
            message.contains("fixture-input") && message.contains("probe refused")
        }));
    }

    #[test]
    fn unknown_readiness_and_empty_declarations_retain_waits() {
        let (ctx, _) = make_static_context("/tmp".into(), None);
        assert!(!ordering_requirements_satisfied(&ctx, &[]));
        let requirement = Requirement::<()>::new("unreadable-input", |_, stage| {
            assert_eq!(stage, RequirementStage::Ordering);
            anyhow::bail!("cannot inspect input")
        });
        let later = Requirement::new("must-not-probe", |_, _| {
            panic!("ordering checks stop at the first unproven requirement");
        });
        assert!(!ordering_requirements_satisfied(
            &ctx,
            &[requirement, later]
        ));
    }

    #[test]
    fn ordering_evidence_is_not_reused_after_a_provider_changes_state() {
        let (ctx, _) = make_static_context("/tmp".into(), None);
        let version = AtomicUsize::new(1);
        let calls = AtomicUsize::new(0);
        let probe = |_: &Context, _: RequirementStage| {
            calls.fetch_add(1, Ordering::SeqCst);
            Ok(RequirementState::Satisfied(version.load(Ordering::SeqCst)))
        };
        assert!(ordering_requirements_satisfied(
            &ctx,
            &[Requirement::new("input", probe).without_evidence()],
        ));
        version.store(2, Ordering::SeqCst);
        let result = process_requirement(&ctx, &Requirement::new("input", probe), |observed| {
            assert_eq!(observed, 2, "execution must recheck after its dependencies");
            Ok(TaskResult::Ok)
        })
        .unwrap();
        assert!(matches!(result, TaskResult::Ok));
        assert_eq!(calls.load(Ordering::SeqCst), 2);
    }

    #[test]
    fn probe_and_consumer_errors_preserve_cancellation() {
        let (ctx, _) = make_static_context("/tmp".into(), None);
        for cancel_probe in [false, true] {
            let cancelled = || {
                anyhow::Error::from(ExecError::Cancelled {
                    command: "fixture probe".into(),
                    result: ExecResult::failure("", "", None),
                })
            };
            let requirement = Requirement::new("cancellable", |_, _| {
                if cancel_probe {
                    Err(cancelled())
                } else {
                    Ok(RequirementState::Satisfied(()))
                }
            });
            let error = process_requirement(&ctx, &requirement, |()| {
                assert!(!cancel_probe, "cancelled inspection must not execute work");
                Err(cancelled())
            })
            .unwrap_err();
            assert!(
                error
                    .downcast_ref::<ExecError>()
                    .is_some_and(ExecError::is_cancelled)
            );
        }
    }
}
