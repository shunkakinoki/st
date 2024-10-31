//! `submit` subcommand.

use crate::{
    ctx::StContext,
    errors::{StError, StResult},
    git::RepositoryExt,
    tree::RemoteMetadata,
};
use clap::Args;
use git2::BranchType;
use nu_ansi_term::Color;
use octocrab::{issues::IssueHandler, models::CommentId, pulls::PullRequestHandler, Octocrab};

/// CLI arguments for the `submit` subcommand.
#[derive(Debug, Clone, Eq, PartialEq, Args)]
pub struct SubmitCmd {
    /// Force the submission of the stack, analogous to `git push --force`.
    #[clap(long, short)]
    force: bool,
}

impl SubmitCmd {
    /// Run the `submit` subcommand.
    pub async fn run(self, mut ctx: StContext<'_>) -> StResult<()> {
        // Establish the GitHub API client.
        let gh_client = Octocrab::builder()
            .personal_token(ctx.cfg.github_token.clone())
            .build()?;
        let (owner, repo) = ctx.owner_and_repository(ctx.tree.remote_name.clone())?;
        let mut pulls = gh_client.pulls(&owner, &repo);
        let mut issues = gh_client.issues(&owner, &repo);

        // Resolve the active stack.
        let stack = ctx.discover_stack()?;

        // Perform pre-flight checks.
        println!("🔍 Checking for closed pull requests...");
        self.pre_flight(&mut ctx, &stack, &mut pulls).await?;

        // Submit the stack.
        println!(
            "\n🐙 Submitting changes to remote `{}`...",
            Color::Blue.paint(&ctx.tree.remote_name)
        );
        self.submit_stack(&mut ctx, &mut pulls, &mut issues, &owner, &repo)
            .await?;

        // Update the stack navigation comments on the PRs.
        println!("\n📝 Updating stack navigation comments...");
        self.update_pr_comments(&mut ctx, gh_client.issues(owner, repo), &stack)
            .await?;

        println!("\n🧙💫 All pull requests up to date.");
        Ok(())
    }

    /// Performs pre-flight checks before submitting the stack.
    async fn pre_flight(
        &self,
        ctx: &mut StContext<'_>,
        stack: &[String],
        pulls: &mut PullRequestHandler<'_>,
    ) -> StResult<()> {
        // Return early if the stack is not restacked or the current working tree is dirty.
        ctx.check_cleanliness(stack)?;

        // Check if any PRs have been closed, and offer to delete them before starting the submission process.
        let num_closed = ctx
            .delete_closed_branches(
                stack.iter().skip(1).cloned().collect::<Vec<_>>().as_slice(),
                pulls,
            )
            .await?;

        if num_closed > 0 {
            println!(
                "Deleted {} closed pull request{}. Run `{}` to re-stack the branches.",
                Color::Red.paint(num_closed.to_string()),
                (num_closed != 1).then_some("s").unwrap_or_default(),
                Color::Blue.paint("st restack")
            );
        }

        Ok(())
    }

    /// Submits the stack of branches to GitHub.
    async fn submit_stack(
        &self,
        ctx: &mut StContext<'_>,
        pulls: &mut PullRequestHandler<'_>,
        issues: &mut IssueHandler<'_>,
        owner: &str,
        repo: &str,
    ) -> StResult<()> {
        let stack = ctx.discover_stack()?;

        // Prompt the user for the assignee of the PR if not initialized.
        Self::prompt_for_assignee(ctx)?;

        // Iterate over the stack and submit PRs.
        for (i, branch) in stack.iter().enumerate().skip(1) {
            let parent = &stack[i - 1];

            // Get the remote to push.
            let remote_name = ctx.tree.remote_name.clone();

            // Determine the target `head` remote.
            let target_remote_name = Self::determine_target_pr_remote(ctx, branch)?;

            // Get the tracked branch.
            let (target_owner, _) = ctx.owner_and_repository(target_remote_name.clone())?;

            // Parse the base branch.
            let base_parent = if target_remote_name.is_empty() {
                parent.clone()
            } else {
                format!("{}:{}", target_owner, parent)
            };

            let tracked_branch = ctx
                .tree
                .get_mut(branch)
                .ok_or_else(|| StError::BranchNotTracked(branch.to_string()))?;

            if let Some(remote_meta) = tracked_branch.remote.as_ref() {
                // If the PR has already been submitted.

                // Grab remote metadata for the pull request.
                let remote_pr = pulls.get(remote_meta.pr_number).await?;

                // Check if the PR base needs to be updated
                if &remote_pr.base.ref_field != parent {
                    // Update the PR base.
                    pulls
                        .update(remote_meta.pr_number)
                        .base(base_parent)
                        .send()
                        .await?;
                    println!(
                        "-> Updated base branch for pull request for branch `{}` to `{}`.",
                        Color::Green.paint(branch),
                        Color::Yellow.paint(parent)
                    );
                }

                // Check if the local branch is ahead of the remote.
                let remote_synced = remote_pr.head.sha
                    == ctx
                        .repository
                        .find_branch(branch, BranchType::Local)?
                        .get()
                        .target()
                        .ok_or(StError::BranchUnavailable)?
                        .to_string();
                if remote_synced {
                    println!(
                        "Branch `{}` is up-to-date with the remote. Skipping push.",
                        Color::Green.paint(branch)
                    );
                    continue;
                }

                // Push the branch to the remote.
                ctx.repository
                    .push_branch(branch, &remote_name, self.force)?;

                // Print success message.
                println!("Updated branch `{}` on remote.", Color::Green.paint(branch));
            } else {
                // If the PR has not been submitted yet.

                // Push the branch to the remote.
                ctx.repository
                    .push_branch(branch, &remote_name, self.force)?;

                // Prompt the user for PR metadata.
                let metadata = Self::prompt_pr_metadata(branch, parent)?;

                // Submit PR.
                let pr_info = pulls
                    .create(metadata.title, branch, base_parent)
                    .body(metadata.body)
                    .draft(metadata.is_draft)
                    .send()
                    .await?;

                // Assign the PR to the user.
                if let Some(assignee) = &ctx.cfg.default_assignee {
                    issues
                        .update(pr_info.number)
                        .assignees(&[assignee.clone()])
                        .send()
                        .await?;
                }

                // Update the tracked branch with the remote information.
                tracked_branch.remote = Some(RemoteMetadata::new(
                    pr_info.number,
                    Some(target_remote_name),
                ));

                // Print success message.
                let pr_link = format!(
                    "https://github.com/{}/{}/pull/{}",
                    owner, repo, pr_info.number
                );
                println!(
                    "Submitted new pull request for branch `{}` @ `{}`",
                    Color::Green.paint(branch),
                    Color::Blue.paint(pr_link)
                );
            }
        }

        Ok(())
    }

    /// Deteremines the target remote for submitting the stack.
    ///
    /// 1. If one of the children of the current branch is already submitted as a PR, use the remote name associated with that PR.
    /// 2. Otherwise, check if the repository is a fork of the default remote. If so, use that remote name.
    /// 3. Otherwise, use the default remote name.
    fn determine_target_pr_remote(ctx: &StContext<'_>, branch: &String) -> StResult<String> {
        // Get the children of the current branch.
        if let Some(tracked_branch) = ctx.tree.branches.get(branch) {
            // If any children have PRs, use their remote name.
            if let Some(Some(remote_name)) = tracked_branch.children.iter().find_map(|child| {
                ctx.tree
                    .branches
                    .get(child)
                    .and_then(|b| b.remote.as_ref())
                    .map(|r| &r.remote_name)
            }) {
                return Ok(remote_name.clone());
            }
        }

        // Check if the repository is a fork of the default remote.
        // let (owner, _) = ctx.owner_and_repository(ctx.tree.remote_name.clone())?;
        // if owner != ctx.cfg.github_user {
        //     return Ok(ctx.tree.remote_name.clone());
        // }

        // Otherwise, use the default remote name.
        Ok(ctx.tree.remote_name.clone())
    }

    /// Updates the comments on a PR with the current stack information.
    async fn update_pr_comments(
        &self,
        ctx: &mut StContext<'_>,
        issue_handler: IssueHandler<'_>,
        stack: &[String],
    ) -> StResult<()> {
        for branch in stack.iter().skip(1) {
            // First, get the necessary data from the tracked branch
            let (pr_number, comment_id) = {
                let tracked_branch = ctx
                    .tree
                    .get(branch)
                    .ok_or_else(|| StError::BranchNotTracked(branch.to_string()))?;

                // Skip branches that are not submitted as PRs
                let Some(remote_meta) = &tracked_branch.remote else {
                    continue;
                };

                (remote_meta.pr_number, remote_meta.comment_id)
            };

            // Render the comment
            let rendered_comment = { Self::render_pr_comment(ctx, branch, stack)? };

            match comment_id {
                Some(id) => {
                    // Update the existing comment.
                    issue_handler
                        .update_comment(CommentId(id), rendered_comment)
                        .await?;
                }
                None => {
                    // Create a new comment.
                    let comment_info = issue_handler
                        .create_comment(pr_number, rendered_comment)
                        .await?;

                    // Get a new mutable reference to the branch and update the comment ID.
                    ctx.tree
                        .get_mut(branch)
                        .expect("Must exist")
                        .remote
                        .as_mut()
                        .expect("Must exist")
                        .comment_id = Some(comment_info.id.0);
                }
            }
        }
        Ok(())
    }

    /// Prompts the user for the assignee of the PR if not initialized. (Set as "none" if not initialized.)
    fn prompt_for_assignee(ctx: &mut StContext<'_>) -> StResult<()> {
        if ctx.cfg.default_assignee.is_none() {
            let assignee = inquire::Text::new("Assignee (default: none)").prompt()?;
            ctx.cfg.default_assignee = Some(assignee.clone());
        }
        Ok(())
    }

    /// Prompts the user for metadata about the PR during the initial submission process.
    fn prompt_pr_metadata(branch_name: &str, parent_name: &str) -> StResult<PRCreationMetadata> {
        let title = inquire::Text::new(
            format!(
                "Title of pull request (`{}` -> `{}`):",
                Color::Green.paint(branch_name),
                Color::Yellow.paint(parent_name)
            )
            .as_str(),
        )
        .prompt()?;
        let body = inquire::Editor::new("Pull request description")
            .with_file_extension(".md")
            .prompt()?;
        let is_draft = inquire::Confirm::new("Is this PR a draft? (default: yes)")
            .with_default(true)
            .prompt()?;

        Ok(PRCreationMetadata {
            title,
            body,
            is_draft,
        })
    }

    /// Renders the PR comment body for the current stack.
    fn render_pr_comment(
        ctx: &StContext<'_>,
        current_branch: &str,
        stack: &[String],
    ) -> StResult<String> {
        let mut comment = String::new();
        comment.push_str("## 📚 $\\text{Stack Overview}$\n\n");
        comment.push_str("Pulls submitted in this stack:\n");

        // Display all branches in the stack.
        for branch in stack.iter().skip(1).rev() {
            let tracked_branch = ctx
                .tree
                .get(branch)
                .ok_or_else(|| StError::BranchNotTracked(branch.to_string()))?;
            if let Some(remote) = &tracked_branch.remote {
                comment.push_str(&format!(
                    "* #{}{}\n",
                    remote.pr_number,
                    (branch == current_branch)
                        .then_some(" 👈")
                        .unwrap_or_default()
                ));
            }
        }
        comment.push_str(format!("* `{}`\n", ctx.tree.trunk_name).as_str());

        comment.push_str(
            "\n_This comment was automatically generated by [`st`](https://github.com/clabby/st)._",
        );
        Ok(comment)
    }
}

/// Metadata about pull request creation.
struct PRCreationMetadata {
    /// Title of the pull request.
    title: String,
    /// Body of the pull request.
    body: String,
    /// Whether or not the pull request is a draft.
    is_draft: bool,
}
