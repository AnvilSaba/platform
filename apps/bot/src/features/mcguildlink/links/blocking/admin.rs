use super::{
    presentation::{render, save},
    repository::{BlockCause, BlockGroup, BlockResult, DatabaseBlockRepository},
};
use crate::features::mcguildlink::permissions::require_moderator;
use crate::{
    app::{AppApplicationContext, AppError, BotDataExt},
    utils::create_safe_allowed_mentions,
};
use poise::CreateReply;
use serenity::all::{MessageFlags, User};

fn format_accounts(group: &BlockGroup) -> String {
    let mut text = String::from("ブロックした Discordアカウント:\n");
    for account in &group.discord {
        text.push_str(&format!(
            "- {} (`{}`){}\n",
            account.name,
            account.user_id,
            if account.user_id == group.root.user_id {
                " (root)"
            } else {
                ""
            }
        ));
    }
    if !group.minecraft.is_empty() {
        text.push_str("\nブロックした Minecraftアカウント:\n");
        for account in &group.minecraft {
            text.push_str(&format!("- {} (`{}`)\n", account.name, account.uuid));
        }
    }
    text
}

/// Discordアカウントの関連グループをブロック・解除・一覧表示します。
#[poise::command(
    slash_command,
    ephemeral,
    guild_only,
    subcommands("block_add", "block_remove", "block_list")
)]
pub async fn block(_: AppApplicationContext<'_>) -> Result<(), AppError> {
    Ok(())
}

/// 指定した Discordアカウントと関連アカウントをブロックします。
#[poise::command(slash_command, ephemeral, guild_only, rename = "add")]
pub async fn block_add(
    ctx: AppApplicationContext<'_>,
    #[description = "ブロックする Discordユーザー"] user: User,
) -> Result<(), AppError> {
    require_moderator(ctx).await?;
    ctx.defer_ephemeral().await?;
    let store = DatabaseBlockRepository::new(ctx.bot_data().database.clone());
    let content = match store.block(user.id.get(), &user.name, BlockCause::Moderator).await? {
        BlockResult::Blocked(group) => format!("Discordアカウントをブロックしました。\n{}", format_accounts(&group)),
        BlockResult::AlreadyBlocked => "その Discordアカウントは既にブロックされています。".into(),
    };
    ctx.send(
        CreateReply::default()
            .ephemeral(true)
            .content(content)
            .allowed_mentions(create_safe_allowed_mentions()),
    )
    .await?;
    Ok(())
}

/// 指定した Discordアカウントのブロックグループ全体を解除します。
#[poise::command(slash_command, ephemeral, guild_only, rename = "remove")]
pub async fn block_remove(
    ctx: AppApplicationContext<'_>,
    #[description = "ブロック解除する Discordユーザー"] user: User,
) -> Result<(), AppError> {
    require_moderator(ctx).await?;
    ctx.defer_ephemeral().await?;
    let store = DatabaseBlockRepository::new(ctx.bot_data().database.clone());
    let content = match store.unblock(user.id.get()).await? {
        Some(group) => format!(
            "Discordアカウントのブロックを解除しました。\n{}",
            format_accounts(&group)
        ),
        None => "その Discordアカウントはブロックされていません。".into(),
    };
    ctx.send(
        CreateReply::default()
            .ephemeral(true)
            .content(content)
            .allowed_mentions(create_safe_allowed_mentions()),
    )
    .await?;
    Ok(())
}

/// ブロック中の関連アカウントグループを表示します。
#[poise::command(slash_command, ephemeral, guild_only, rename = "list")]
pub async fn block_list(ctx: AppApplicationContext<'_>) -> Result<(), AppError> {
    require_moderator(ctx).await?;
    ctx.defer_ephemeral().await?;
    let store = DatabaseBlockRepository::new(ctx.bot_data().database.clone());
    let groups = store.list().await?;
    if groups.is_empty() {
        ctx.send(
            CreateReply::default()
                .ephemeral(true)
                .content("ブロックされている Discordアカウントはありません。")
                .allowed_mentions(create_safe_allowed_mentions()),
        )
        .await?;
        return Ok(());
    }
    let snapshot = save(ctx.interaction.id.get(), ctx.author().id.get(), groups);
    ctx.send(
        CreateReply::default()
            .ephemeral(true)
            .flags(MessageFlags::IS_COMPONENTS_V2)
            .components(render(ctx.interaction.id.get(), &snapshot, 0))
            .allowed_mentions(create_safe_allowed_mentions()),
    )
    .await?;
    Ok(())
}
