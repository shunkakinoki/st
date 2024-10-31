//! `config` subcommand.

use crate::{ctx::StContext, errors::StResult};

#[derive(Debug, Clone, Eq, PartialEq, clap::Args)]
pub struct SetCmd {
    /// The name of the remote to set.
    remote_name: Option<String>,
    /// The username of the default assignee to use for new PRs.
    default_assignee: Option<String>,
}

impl SetCmd {
    /// Run the `config` subcommand to force or allow configuration editing.
    pub fn run(self, mut ctx: StContext<'_>) -> StResult<()> {
        if let Some(remote_name) = self.remote_name {
            ctx.tree.remote_name = remote_name;
        }

        if let Some(default_assignee) = self.default_assignee {
            ctx.cfg.default_assignee = Some(default_assignee);
        }

        Ok(())
    }
}
