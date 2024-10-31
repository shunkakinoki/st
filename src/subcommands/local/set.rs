//! `config` subcommand.

use crate::{ctx::StContext, errors::StResult};

#[derive(Debug, Clone, Eq, PartialEq, clap::Args)]
pub struct SetCmd {
    #[command(subcommand)]
    command: SetCommands,
}

#[derive(Debug, Clone, Eq, PartialEq, clap::Subcommand)]
pub enum SetCommands {
    /// Set the remote name
    Remote(RemoteArgs),
    /// Set the default assignee
    Assignee(AssigneeArgs),
}

#[derive(Debug, Clone, Eq, PartialEq, clap::Args)]
pub struct RemoteArgs {
    /// The name of the remote to set
    pub name: String,
}

#[derive(Debug, Clone, Eq, PartialEq, clap::Args)]
pub struct AssigneeArgs {
    /// The username of the default assignee
    pub username: String,
}

impl SetCmd {
    /// Run the `set` subcommand to force or allow configuration editing.
    pub fn run(self, mut ctx: StContext<'_>) -> StResult<()> {
        match self.command {
            SetCommands::Remote(args) => {
                ctx.tree.remote_name = args.name;
            }
            SetCommands::Assignee(args) => {
                ctx.cfg.default_assignee = Some(args.username);
            }
        }

        Ok(())
    }
}
