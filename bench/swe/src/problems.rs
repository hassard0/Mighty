//! Fixed 10-problem SWE-bench Verified smoke subset.
//!
//! The smoke subset is hand-picked for diversity (see
//! `SMOKE_PROBLEMS.md` at the crate root). Every entry pins
//! the instance ID, upstream repo, the buggy commit (so we
//! can `git checkout` a clean reproducible workspace), and
//! the failing-tests selector for `pytest`.
//!
//! When `--all` is passed on the CLI the harness ignores this
//! subset and pulls every instance from the dataset cache
//! (`src/dataset.rs`).

use serde::{Deserialize, Serialize};

/// Bump when the curated smoke list changes so historical
/// results stay distinguishable.
pub const SMOKE_SUBSET_VERSION: u32 = 1;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SmokeProblem {
    /// `princeton-nlp/SWE-bench_Verified` instance ID.
    pub instance_id: &'static str,
    /// GitHub `owner/repo` slug.
    pub repo: &'static str,
    /// Buggy base commit (the agent diffs against this).
    pub base_commit: &'static str,
    /// SWE-bench `difficulty` field — informational only.
    pub difficulty: Difficulty,
    /// Plain-English problem statement (truncated; the full text
    /// is fetched from the dataset at run time and overrides this).
    pub statement_preview: &'static str,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum Difficulty {
    Easy,
    Medium,
    Hard,
}

/// The fixed v1 smoke subset.
///
/// The base commits here are taken from SWE-bench Verified's
/// `base_commit` field for each instance. Network failures
/// resolving them are reported as `Skipped { reason }` rather
/// than counted as failures.
pub const SMOKE_PROBLEMS: &[SmokeProblem] = &[
    SmokeProblem {
        instance_id: "sympy__sympy-20590",
        repo: "sympy/sympy",
        base_commit: "cffd4e0f86fefd4802349a9f9b19ed70934ea354",
        difficulty: Difficulty::Easy,
        statement_preview: "Symbol instances have __dict__ since 1.7",
    },
    SmokeProblem {
        instance_id: "django__django-11999",
        repo: "django/django",
        base_commit: "84633905273fc916e3d17883810d9969c03f73c2",
        difficulty: Difficulty::Easy,
        statement_preview: "Cannot override get_FOO_display() in Django 2.2+",
    },
    SmokeProblem {
        instance_id: "astropy__astropy-14365",
        repo: "astropy/astropy",
        base_commit: "7269fa3e33e8d02485a647da91a5a2a60a06af61",
        difficulty: Difficulty::Easy,
        statement_preview: "ascii.qdp Table format assumes commands are upper case",
    },
    SmokeProblem {
        instance_id: "scikit-learn__scikit-learn-13779",
        repo: "scikit-learn/scikit-learn",
        base_commit: "b34751b7ed02b2cfcc36037fb729d4360480a299",
        difficulty: Difficulty::Easy,
        statement_preview: "Voting estimator fails at fit if any estimator is None",
    },
    SmokeProblem {
        instance_id: "pytest-dev__pytest-7373",
        repo: "pytest-dev/pytest",
        base_commit: "7b77fc086aab8b3a8ebc890200371884555eea1e",
        difficulty: Difficulty::Easy,
        statement_preview: "Incorrect caching of skipif/xfail string condition evaluation",
    },
    SmokeProblem {
        instance_id: "matplotlib__matplotlib-23476",
        repo: "matplotlib/matplotlib",
        base_commit: "33a0599711d26dc2b79f851c6daed4947df7c167",
        difficulty: Difficulty::Medium,
        statement_preview: "DPI of a figure is doubled after unpickling on M1 Mac",
    },
    SmokeProblem {
        instance_id: "psf__requests-1142",
        repo: "psf/requests",
        base_commit: "a0df2cbb10419037d11d04352b3175405ab52941",
        difficulty: Difficulty::Medium,
        statement_preview: "request.method is being overwritten in inner method",
    },
    SmokeProblem {
        instance_id: "pallets__flask-4045",
        repo: "pallets/flask",
        base_commit: "d8c37f43724cd9fb0870f77877b7c4c7e38a19e0",
        difficulty: Difficulty::Medium,
        statement_preview: "Raise error when blueprint name contains a dot",
    },
    SmokeProblem {
        instance_id: "django__django-13447",
        repo: "django/django",
        base_commit: "0456d3e42795481a186db05719300691fe2a1029",
        difficulty: Difficulty::Hard,
        statement_preview: "Added model class to app_list context",
    },
    SmokeProblem {
        instance_id: "sympy__sympy-13971",
        repo: "sympy/sympy",
        base_commit: "84c125972ad535b2dfb245f8d311d347b45e5b8a",
        difficulty: Difficulty::Hard,
        statement_preview: "Display of SeqFormula() uses LaTeX backslashes incorrectly",
    },
];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn smoke_subset_is_10() {
        assert_eq!(SMOKE_PROBLEMS.len(), 10);
    }

    #[test]
    fn difficulty_mix_is_5_3_2() {
        let easy = SMOKE_PROBLEMS
            .iter()
            .filter(|p| p.difficulty == Difficulty::Easy)
            .count();
        let med = SMOKE_PROBLEMS
            .iter()
            .filter(|p| p.difficulty == Difficulty::Medium)
            .count();
        let hard = SMOKE_PROBLEMS
            .iter()
            .filter(|p| p.difficulty == Difficulty::Hard)
            .count();
        assert_eq!((easy, med, hard), (5, 3, 2));
    }

    #[test]
    fn instance_ids_unique() {
        let mut ids: Vec<_> = SMOKE_PROBLEMS.iter().map(|p| p.instance_id).collect();
        ids.sort();
        ids.dedup();
        assert_eq!(ids.len(), SMOKE_PROBLEMS.len());
    }
}
