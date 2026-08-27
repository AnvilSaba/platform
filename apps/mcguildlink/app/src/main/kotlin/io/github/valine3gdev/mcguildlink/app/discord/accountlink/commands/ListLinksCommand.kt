package io.github.valine3gdev.mcguildlink.app.discord.accountlink.commands

import dev.kord.common.entity.Snowflake
import dev.kord.core.Kord
import dev.kord.core.behavior.interaction.respondEphemeral
import dev.kord.rest.builder.interaction.string
import dev.kord.rest.builder.interaction.subCommand
import dev.kord.rest.builder.interaction.user
import io.github.valine3gdev.mcguildlink.app.discord.accountlink.interactions.respondAllAccountLinksPaginated
import io.github.valine3gdev.mcguildlink.app.discord.accountlink.interactions.respondDiscordAccountLinksPaginated
import io.github.valine3gdev.mcguildlink.app.discord.accountlink.interactions.respondMinecraftAccountLinksPaginated
import io.github.valine3gdev.mcguildlink.app.discord.util.guardUserRole
import io.github.valine3gdev.mcguildlink.app.discord.util.handleSub
import io.github.valine3gdev.mcguildlink.app.service.AccountLinkService
import kotlin.uuid.Uuid


private const val USER_OPTION_KEY = "user"
private const val UUID_OPTION_KEY = "uuid"

/**
 * 紐付け済みアカウント一覧を検索・表示する管理者向けコマンドを登録します。
 */
context(accountLinkService: AccountLinkService)
internal suspend fun Kord.installListLinksCommand(guildId: Snowflake, moderatorRole: Snowflake) {
    createGuildChatInputCommand(
        guildId,
        "links",
        "紐付け済みアカウントの一覧を表示します。"
    ) {
        disableCommandInGuilds()

        subCommand("discord", "指定した Discordアカウントの紐付け一覧を表示します。") {
            user(USER_OPTION_KEY, "一覧表示する Discordユーザー") {
                required = true
            }
        }

        subCommand("minecraft", "指定した Minecraft UUID の紐付け一覧を表示します。") {
            string(UUID_OPTION_KEY, "一覧表示する Minecraft UUID") {
                required = true
            }
        }

        subCommand("all", "全ての紐付け一覧を表示します。")
    }.handleSub("discord") {
        guardUserRole(moderatorRole) || return@handleSub

        val target = interaction.command.users[USER_OPTION_KEY] ?: run {
            interaction.respondEphemeral {
                content = "対象の Discordユーザーを指定してください。"
            }
            return@handleSub
        }

        val links = accountLinkService.listLinksByDiscord(target.id.value)
        if (links.isEmpty()) {
            interaction.respondEphemeral {
                content = "その Discordアカウントに紐付けられている Minecraftアカウントはありません。"
            }
            return@handleSub
        }

        respondDiscordAccountLinksPaginated(interaction, links)
    }.handleSub("minecraft") {
        guardUserRole(moderatorRole) || return@handleSub

        val uuidText = interaction.command.strings[UUID_OPTION_KEY] ?: run {
            interaction.respondEphemeral {
                content = "対象の Minecraft UUID を指定してください。"
            }
            return@handleSub
        }

        val uuid = runCatching { Uuid.parseHexDash(uuidText) }.getOrNull() ?: run {
            interaction.respondEphemeral {
                content = "Minecraft UUID は `xxxxxxxx-xxxx-xxxx-xxxx-xxxxxxxxxxxx` 形式で指定してください。"
            }
            return@handleSub
        }

        val links = accountLinkService.listLinksByMinecraft(uuid)
        if (links.isEmpty()) {
            interaction.respondEphemeral {
                content = "その Minecraftアカウントに紐付けられている Discordアカウントはありません。"
            }
            return@handleSub
        }

        respondMinecraftAccountLinksPaginated(interaction, links)
    }.handleSub("all") {
        guardUserRole(moderatorRole) || return@handleSub

        val links = accountLinkService.listAllLinks()
        if (links.isEmpty()) {
            interaction.respondEphemeral {
                content = "紐付け済みアカウントはありません。"
            }
            return@handleSub
        }

        respondAllAccountLinksPaginated(interaction, links)
    }
}
