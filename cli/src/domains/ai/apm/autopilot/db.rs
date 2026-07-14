//! Copilot App workflow database discovery for the APM autopilot fixup.

use crate::engine::Context;

/// Result of locating the Copilot App `SQLite` database and a Python
/// interpreter to drive it.
///
/// Both
/// [`apply_workflow_autopilot_fixup`](super::apply_workflow_autopilot_fixup)
/// and
/// [`snapshot_desired_apm_workflow_ids`](super::snapshot_desired_apm_workflow_ids)
/// need the same four things -- the `~/.copilot/data.db` path, proof it exists,
/// a UTF-8 rendering of that path for the script argv, and a `python3`/`python`
/// interpreter -- but they report the failure modes differently: the fixup
/// warns loudly (it is re-asserting user-visible state) while the snapshot stays
/// at debug level (a missing snapshot must never manufacture a false
/// "set N workflow(s)" line). This enum carries the probe outcome and the data
/// each caller needs to format its own message, so the shared mechanism lives in
/// one place while the divergent logging stays with the callers.
#[derive(Debug, PartialEq, Eq)]
pub(super) enum WorkflowDbProbe {
    /// The database exists and an interpreter is available.
    Ready {
        /// The interpreter to invoke (`python3` preferred, else `python`).
        python: &'static str,
        /// UTF-8 rendering of the database path for the script argv.
        db_str: String,
    },
    /// The database file does not exist yet.
    DbMissing {
        /// Display path of the absent database.
        path: String,
    },
    /// The database path could not be stat'd.
    DbStatError {
        /// Display path of the database.
        path: String,
        /// The stat error, rendered for logging.
        error: String,
    },
    /// The database path is not valid UTF-8 and cannot be passed to the script.
    DbPathNotUtf8 {
        /// Display path of the database.
        path: String,
    },
    /// Neither `python3` nor `python` is on `PATH`.
    PythonMissing,
}

/// Locate the Copilot App database and a Python interpreter.
///
/// Shared preamble for
/// [`apply_workflow_autopilot_fixup`](super::apply_workflow_autopilot_fixup)
/// and
/// [`snapshot_desired_apm_workflow_ids`](super::snapshot_desired_apm_workflow_ids);
/// see [`WorkflowDbProbe`] for why the outcome is returned rather than logged
/// here.
pub(super) fn probe_workflow_db(ctx: &Context) -> WorkflowDbProbe {
    let system = ctx.system();
    let db = system.home().join(".copilot").join("data.db");
    match db.try_exists() {
        Ok(true) => {}
        Ok(false) => {
            return WorkflowDbProbe::DbMissing {
                path: db.display().to_string(),
            };
        }
        Err(e) => {
            return WorkflowDbProbe::DbStatError {
                path: db.display().to_string(),
                error: e.to_string(),
            };
        }
    }

    let Some(db_str) = db.to_str() else {
        return WorkflowDbProbe::DbPathNotUtf8 {
            path: db.display().to_string(),
        };
    };

    let python = if system.which("python3") {
        "python3"
    } else if system.which("python") {
        "python"
    } else {
        return WorkflowDbProbe::PythonMissing;
    };

    WorkflowDbProbe::Ready {
        python,
        db_str: db_str.to_string(),
    }
}
