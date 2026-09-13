mod admin;
mod auth;
mod discord_management;
mod honeypot;
mod message_cache_handler;
mod message_logging;
mod pin;
mod question;
mod thread_auto_invite;

use std::borrow::Cow;

#[cfg(debug_assertions)]
use crate::app::{AppContext, AppError};
use crate::{
    app::{AppCommand, config::AppConfig},
    core::BotEventHandlers,
    features::{
        auth::{AutoKickEventHandler, KeywordAuthEventHandler},
        honeypot::handle_honeypot_event,
        message_cache_handler::MessageCacheHandler,
        message_logging::MessageLoggingEventHandler,
        question::handle_question_event,
        thread_auto_invite::handle_thread_auto_invite_event,
    },
};

pub fn event_handlers(config: &AppConfig) -> BotEventHandlers {
    BotEventHandlers::new()
        .add(handle_honeypot_event)
        .add(MessageLoggingEventHandler::new())
        .add(handle_thread_auto_invite_event)
        .add(handle_question_event)
        .add(KeywordAuthEventHandler::new())
        .add(AutoKickEventHandler::new())
        .add(MessageCacheHandler::new(config.message_cache.disabled))
}

#[cfg(debug_assertions)]
#[poise::command(prefix_command)]
pub async fn register(ctx: AppContext<'_>) -> Result<(), AppError> {
    poise::builtins::register_application_commands_buttons(ctx).await?;
    Ok(())
}

pub fn commands() -> Vec<AppCommand> {
    let commands = [
        auth::create_keyword_button,
        question::question,
        pin::pin,
        discord_management::bind,
        discord_management::role_apply,
        discord_management::role_export,
        discord_management::role_plan,
        admin::reload_config,
        thread_auto_invite::invite_thread,
        thread_auto_invite::add_invite_role,
        thread_auto_invite::remove_invite_role,
    ]
    .to_vec();
    #[cfg(debug_assertions)]
    let commands = {
        let mut commands = commands;
        commands.push(register);
        commands
    };

    build_commands(commands)
}

fn alias_command(base: fn() -> AppCommand, name: Cow<'static, str>) -> AppCommand {
    let mut command = base();
    command.name = name;
    command.aliases = (&[]).into();
    command.context_menu_action = None;
    command.context_menu_name = None;
    command
}

fn build_commands(commands: Vec<fn() -> AppCommand>) -> Vec<AppCommand> {
    commands
        .into_iter()
        .flat_map(|cmd| {
            let base = cmd();
            let aliases = base.aliases.clone();
            std::iter::once(base)
                .chain(aliases.iter().map(move |a| alias_command(cmd, a.clone())))
                .collect::<Vec<_>>()
        })
        .collect()
}
