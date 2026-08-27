package io.github.valine3gdev.mcguildlink.app.discord.accountlink.commands

import dev.kord.common.entity.*
import dev.kord.core.Kord
import dev.kord.core.behavior.channel.createMessage
import dev.kord.core.behavior.interaction.response.respond
import dev.kord.core.builder.components.emoji
import dev.kord.core.entity.ReactionEmoji
import dev.kord.rest.builder.component.actionRow
import dev.kord.rest.builder.message.container
import io.github.valine3gdev.mcguildlink.app.discord.accountlink.LIST_LINK_BUTTON_ID
import io.github.valine3gdev.mcguildlink.app.discord.accountlink.START_LINK_BUTTON_ID
import io.github.valine3gdev.mcguildlink.app.discord.util.guardBotPermissions
import io.github.valine3gdev.mcguildlink.app.discord.util.guardUserRole
import io.github.valine3gdev.mcguildlink.app.discord.util.handleRoot


/**
 * 紐付け開始パネルを投稿する管理者向けコマンドを登録します。
 */
internal suspend fun Kord.installCreatePanelCommand(guildId: Snowflake, moderatorRole: Snowflake) {
    createGuildChatInputCommand(guildId, "create_panel", "紐付けを開始するためのパネルを送信します。") {
        disableCommandInGuilds()
    }.handleRoot {
        guardUserRole(moderatorRole) || return@handleRoot
        guardBotPermissions(Permission.ViewChannel + Permission.SendMessages) || return@handleRoot

        val deferred = interaction.deferEphemeralResponse()

        interaction.channel.createMessage {
            flags = MessageFlags(MessageFlag.IsComponentsV2)

            container {
                textDisplay(
                    """
                            Minecraftアカウントと Discordアカウントを紐付けます。
                            「MCアカウントと紐付ける」ボタンを押して、指示に従ってください。
                            「紐付けられたアカウントを確認する」ボタンを押すと、現在紐付けられているアカウントの一覧を確認できます。
                        """.trimIndent()
                )

                separator(SeparatorSpacingSize.Large)

                actionRow {
                    interactionButton(
                        ButtonStyle.Primary,
                        START_LINK_BUTTON_ID,
                    ) {
                        emoji(ReactionEmoji.Unicode("\uD83D\uDD17"))
                        label = "MCアカウントと紐付ける"
                    }

                    interactionButton(
                        ButtonStyle.Secondary,
                        LIST_LINK_BUTTON_ID,
                    ) {
                        emoji(ReactionEmoji.Unicode("\uD83D\uDCCB"))
                        label = "紐付けられたアカウントを確認する"
                    }
                }
            }
        }

        deferred.respond {
            content = "パネルを作成しました！"
        }
    }
}
