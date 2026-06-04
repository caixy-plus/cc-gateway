//! Shared inbound chat flow: parse with [`CommandRouter`], execute with [`ChatCommandExecutor`].

use anyhow::Result;

use crate::command::router::CommandRouter;
use crate::session::channel_command::{
    ChatCommandContext, ChatCommandExecutor, ChatCommandOutcome,
};

pub(crate) async fn route_and_execute(
    router: &CommandRouter,
    executor: &ChatCommandExecutor,
    context: &mut ChatCommandContext,
    message: &str,
) -> Result<ChatCommandOutcome> {
    let action = router.route(message).await;
    executor.execute(context, action).await
}
