//! `status` subcommand.

use crate::{
    ctx::StContext,
    errors::{StError, StResult},
};
use clap::Args;
use cli_table::{Cell, Style, Table};
use octocrab::{models::IssueState, Octocrab};

/// CLI arguments for the `status` subcommand.
#[derive(Debug, Clone, Eq, PartialEq, Args)]
pub struct StatusCmd {}

impl StatusCmd {
    /// Run the `status` subcommand.
    pub async fn run(self, mut ctx: StContext<'_>) -> StResult<()> {
        // Establish the GitHub API client.
        let gh_client = Octocrab::builder()
            .personal_token(ctx.cfg.github_token.clone())
            .build()?;

        let current_stack = ctx.discover_stack()?;

        let mut rows = vec![];
        for branch in current_stack.into_iter() {
            let tracked_branch = ctx
                .tree
                .get(&branch)
                .ok_or_else(|| StError::BranchNotTracked(branch.clone()))?
                .clone();
            let mut row = Vec::with_capacity(4);

            row.push(branch.clone());
            row.push(
                tracked_branch
                    .parent
                    .clone()
                    .unwrap_or("n/a: trunk branch".to_string()),
            );

            if let Some(remote) = tracked_branch.remote {
                // Override the remote name if provided.
                ctx.set_remote_name(remote.remote_name.clone());

                if !ctx.remote_names()?.is_empty() {
                    row.push(remote.remote_name.unwrap_or("origin".to_string()));
                }

                let (owner, repo) = ctx.owner_and_repository()?;
                let pulls = gh_client.pulls(&owner, &repo);

                let pr_info = pulls.get(remote.pr_number).await?;
                let is_draft = pr_info.draft.unwrap_or_default();
                let is_merged = pr_info.merged_at.is_some();
                let is_closed = pr_info
                    .state
                    .map_or(true, |s| matches!(s, IssueState::Closed));

                row.push(if ctx.needs_restack(&branch)? {
                    "🔴 Needs Restack".to_string()
                } else {
                    "✅ Restacked".to_string()
                });

                if is_draft {
                    row.push("📝 Draft".to_string());
                } else if is_merged {
                    row.push("✅ Merged".to_string());
                } else if is_closed {
                    row.push("❌ Closed".to_string());
                } else {
                    row.push("🔍 In Review".to_string());
                }
            } else {
                row.push("🚧 Not Submitted".to_string());
            }

            rows.push(row);
        }

        let mut headers = vec![
            "Branch Name".cell().bold(true),
            "Parent Branch".cell().bold(true),
        ];
        if !ctx.remote_names()?.is_empty() {
            headers.push("Remote".cell().bold(true));
        }
        headers.push("Stack Status".cell().bold(true));
        headers.push("PR Status".cell().bold(true));

        let table = rows.table().title(headers).bold(true);
        println!("{}", table.display().expect("Failed to display table"));
        Ok(())
    }
}
