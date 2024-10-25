//! `create` subcommand.

use crate::{
    ctx::StContext,
    errors::{StError, StResult},
    git::RepositoryExt,
};
use clap::Args;
use git2::IndexAddOption;
use nu_ansi_term::Color;

/// CLI arguments for the `create` subcommand.
#[derive(Debug, Clone, Eq, PartialEq, Args)]
pub struct CreateCmd {
    /// Name of the new branch to create.
    #[clap(index = 1)]
    branch_name: Option<String>,
    /// Stage all changes before creating branch
    #[clap(short = 'a', long = "all")]
    all: bool,
    /// Stage only tracked files before creating branch
    #[clap(short, long = "update")]
    update: bool,
    /// Specify a commit message
    #[clap(short, long, requires = "all", conflicts_with = "update")]
    message: Option<String>,
}

impl CreateCmd {
    /// Run the `create` subcommand.
    pub fn run(self, mut ctx: StContext<'_>) -> StResult<()> {
        // Gather metadata about the current branch.
        let current_branch = ctx.repository.current_branch()?;
        let current_branch_head = current_branch.get().peel_to_commit()?;
        let current_branch_name = ctx.repository.current_branch_name()?;

        // Prompt the user for the name of their new branch, or use the provided name.
        let new_branch_name = match self.branch_name {
            Some(name) => name,
            None => inquire::Text::new("Name of new branch:").prompt()?,
        };

        // Stage changes if requested
        if self.all || self.update {
            let message = self.message.ok_or(StError::CommitMessageRequired)?;

            // Get the index.
            let mut index = ctx.repository.index()?;

            // Stage changes based on flag.
            if self.all {
                index.add_all(vec!["*"], IndexAddOption::DEFAULT, None)?;
            } else if self.update {
                index.update_all(vec!["*"], None)?;
            }
            index.write()?;

            // Create the commit.
            let sig = ctx.repository.signature()?;
            // Get the tree.
            let tree_id = ctx.repository.index()?.write_tree()?;
            let tree = ctx.repository.find_tree(tree_id)?;
            // Get the parent commit.
            let parent_commit = ctx.repository.head()?.peel_to_commit()?;
            // Create the commit.
            ctx.repository
                .commit(Some("HEAD"), &sig, &sig, &message, &tree, &[&parent_commit])?;
        }

        // Check if the working tree is clean.
        if !ctx.repository.is_working_tree_clean()? {
            return Err(crate::errors::StError::WorkingTreeDirty);
        }

        // Attempt to create the new branch.
        ctx.repository
            .branch(&new_branch_name, &current_branch_head, false)?;
        ctx.repository.checkout_branch(&new_branch_name)?;

        // Insert the new branch into the stack tree.
        ctx.tree.insert(
            &current_branch_name,
            &current_branch_head.id().to_string(),
            &new_branch_name,
        )?;

        println!(
            "Successfully created and tracked new branch `{}` on top of `{}`",
            Color::Blue.paint(&new_branch_name),
            Color::Blue.paint(&current_branch_name)
        );
        Ok(())
    }
}
