//! Weekly-project verification: clone the learner's repo, review it with the
//! Claude CLI against that week's content, and parse the verdict.

use std::sync::mpsc::Sender;

use crate::github;
use crate::mentor::{self, CliConfig, JobResult};
use crate::models::VerificationJson;

/// Upper bound on source text pasted into the fallback prompt.
pub const SOURCE_CAP_BYTES: usize = 180_000;

pub struct VerifyRequest {
    pub weekly_project_id: i64,
    pub github_url: String,
    pub week_subskills: String,
    pub description: String,
    pub acceptance: String,
}

/// Clone, review and report back on a background thread.
pub fn spawn_verify(tx: Sender<JobResult>, cfg: CliConfig, req: VerifyRequest) {
    std::thread::spawn(move || {
        let (tmp, sha) = match github::clone_repo(&cfg.git_path, &req.github_url) {
            Ok(v) => v,
            Err(e) => {
                let _ = tx.send(JobResult::Verify {
                    weekly_project_id: req.weekly_project_id,
                    github_url: req.github_url,
                    commit_sha: String::new(),
                    report_md: String::new(),
                    outcome: Err(e.to_string()),
                });
                return;
            }
        };
        let repo = github::repo_dir(&tmp);
        let prompt = mentor::prompt_verify(&req.week_subskills, &req.description, &req.acceptance);

        // Preferred path: run the CLI inside the clone so it reads files itself.
        let mut report = mentor::run_cli(&cfg, &prompt, Some(&repo));

        // Fallback: paste filtered sources into the prompt.
        if report.as_ref().map(|r| r.trim().is_empty()).unwrap_or(true) {
            let sources = github::gather_sources(&repo, SOURCE_CAP_BYTES).unwrap_or_default();
            if !sources.trim().is_empty() {
                let fallback = format!(
                    "{prompt}\n\nThe repository source files follow.\n{sources}"
                );
                report = mentor::run_cli(&cfg, &fallback, None);
            }
        }

        let result = match report {
            Ok(text) => {
                let parsed = mentor::extract_last_json(&text)
                    .and_then(|j| serde_json::from_str::<VerificationJson>(j).ok());
                match parsed {
                    Some(mut v) => {
                        v.score = v.score.clamp(0, 100);
                        if v.verdict.trim().is_empty() {
                            v.verdict = "unknown".to_string();
                        }
                        JobResult::Verify {
                            weekly_project_id: req.weekly_project_id,
                            github_url: req.github_url,
                            commit_sha: sha,
                            report_md: text,
                            outcome: Ok(v),
                        }
                    }
                    // Keep the prose review even when the JSON block is missing.
                    None => JobResult::Verify {
                        weekly_project_id: req.weekly_project_id,
                        github_url: req.github_url,
                        commit_sha: sha,
                        report_md: text,
                        outcome: Err(
                            "review completed but no JSON verdict block was found".to_string()
                        ),
                    },
                }
            }
            Err(e) => JobResult::Verify {
                weekly_project_id: req.weekly_project_id,
                github_url: req.github_url,
                commit_sha: sha,
                report_md: String::new(),
                outcome: Err(e.to_string()),
            },
        };

        drop(tmp); // remove the clone
        let _ = tx.send(result);
    });
}
