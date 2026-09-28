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

pub fn commands() -> Vec<AppCommand> {
    build_commands(
        [
            auth::create_keyword_button,
            question::question,
            pin::pin,
            discord_management::role_export,
            discord_management::role_plan,
            admin::reload_config,
            thread_auto_invite::invite_thread,
            thread_auto_invite::add_invite_role,
            thread_auto_invite::remove_invite_role,
        ]
        .to_vec(),
    )
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn command_registration_keeps_existing_commands_and_adds_role_management() {
        let commands = commands();
        let names = commands
            .iter()
            .map(|command| command.name.to_string())
            .collect::<Vec<_>>();

        for management_command in commands
            .iter()
            .filter(|command| matches!(command.name.as_ref(), "role_export" | "role_plan"))
        {
            assert!(management_command.owners_only);
            assert!(management_command.guild_only);
            assert!(management_command.ephemeral);
        }

        for expected in [
            "create_keyword_button",
            "question",
            "pin",
            "role_export",
            "role_plan",
            "reload_config",
            "invite_thread",
            "add_invite_role",
            "remove_invite_role",
        ] {
            assert!(names.iter().any(|name| name == expected), "missing command: {expected}");
        }
    }
}
