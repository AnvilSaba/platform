package io.github.valine3gdev.mcguildlink.app.discord.accountlink.interactions

import dev.kord.common.entity.ButtonStyle
import dev.kord.common.entity.SeparatorSpacingSize
import dev.kord.core.behavior.interaction.respondEphemeral
import dev.kord.core.builder.components.emoji
import dev.kord.core.entity.ReactionEmoji
import dev.kord.core.entity.User
import dev.kord.rest.builder.component.ContainerBuilder
import dev.kord.rest.builder.component.actionRow
import dev.kord.rest.builder.component.section
import dev.kord.rest.builder.component.separator
import io.github.oshai.kotlinlogging.KotlinLogging
import io.github.valine3gdev.mcguildlink.app.config.BotConfig
import io.github.valine3gdev.mcguildlink.app.discord.accountlink.LIST_LINK_BUTTON_ID
import io.github.valine3gdev.mcguildlink.app.discord.accountlink.START_LINK_BUTTON_ID
import io.github.valine3gdev.mcguildlink.app.discord.accountlink.UNLINK_BUTTON_ID_PREFIX
import io.github.valine3gdev.mcguildlink.app.discord.registry.EphemeralPagination
import io.github.valine3gdev.mcguildlink.app.discord.registry.InteractionRegistry
import io.github.valine3gdev.mcguildlink.app.discord.registry.PaginationSnapshotPage
import io.github.valine3gdev.mcguildlink.app.discord.registry.createLinkedCustomIdString
import io.github.valine3gdev.mcguildlink.app.service.AccountLinkService
import io.github.valine3gdev.mcguildlink.app.service.dto.LinkRequestResult
import io.github.valine3gdev.mcguildlink.app.service.dto.MinecraftAccountInfo
import io.github.valine3gdev.mcguildlink.app.service.dto.toHeadAvatarUrl
import io.github.valine3gdev.mcguildlink.app.util.getLinkedMinecraftAccounts
import io.github.valine3gdev.mcguildlink.app.util.getOrCreateLinkRequest


private val logger = KotlinLogging.logger {}

private val linkedAccountsPagination = EphemeralPagination(
    prefix = "list_link_page_button:",
    pageSize = 5,
    renderPage = { user, page -> applyLinkedAccountsPage(page, user) },
)

/**
 * 紐付け開始ボタン、紐付け一覧ボタン、一覧ページ送りを登録します。
 */
context(config: BotConfig, accountLinkService: AccountLinkService)
internal fun InteractionRegistry.installAccountLinkButtons() {
    interactionButton(START_LINK_BUTTON_ID) {
        when (val request = accountLinkService.getOrCreateLinkRequest(interaction.user)) {
            is LinkRequestResult.Success -> {
                logger.debug { "Requesting link code for user ${interaction.user.tag} (${interaction.user.id})" }
                interaction.respondEphemeral {
                    content = """
                        Minecraft 1.21.11 で以下のサーバーに接続し、表示される入力欄にコードを入力してください。
                        
                        サーバーアドレス:
                        ```
                        ${config.displayServerAddress}
                        ```
                        コード:
                        ```
                        ${request.code}
                        ```
                    """.trimIndent()
                }
            }

            LinkRequestResult.Blocked -> interaction.respondEphemeral {
                content = "この Discordアカウントはブロックされているため、紐付けを開始できません。"
            }
        }
    }

    interactionButton(LIST_LINK_BUTTON_ID) {
        val accounts = accountLinkService.getLinkedMinecraftAccounts(interaction.user)
        if (accounts.isEmpty()) {
            interaction.respondEphemeral {
                content = "あなたの Discordアカウントに紐付けられた Minecraftアカウントはありません。"
            }
            return@interactionButton
        }

        linkedAccountsPagination.respondEphemeralPaginatedSnapshot(interaction, accounts)
    }

    linkedAccountsPagination.installPaginationButton()
}

/**
 * 自分に紐付いた Minecraft アカウント一覧ページを描画します。
 */
private fun ContainerBuilder.applyLinkedAccountsPage(
    page: PaginationSnapshotPage<MinecraftAccountInfo>,
    user: User,
) {
    textDisplay(
        """
            ## 紐付けられたアカウント
            以下の Minecraftアカウントがあなたの Discordアカウントに紐付けられています。
            紐付けを解除したいアカウントがある場合は、各アカウントの「解除」ボタンを押してください。
        """.trimIndent()
    )

    separator {
        divider = false
    }

    page.entries.forEach { minecraft ->
        appendMinecraftAccount(user, minecraft)
    }
}

/**
 * Minecraft アカウント 1 件分の表示と解除ボタンを描画します。
 */
private fun ContainerBuilder.appendMinecraftAccount(user: User, minecraft: MinecraftAccountInfo) {
    val uuid = minecraft.uuid

    separator(SeparatorSpacingSize.Large)

    section {
        textDisplay(
            """
                - 名前: **${minecraft.lastKnownName}**
                - UUID: `${minecraft.uuid}`
            """.trimIndent()
        )
        thumbnailAccessory {
            url = minecraft.toHeadAvatarUrl()
        }
    }

    actionRow {
        interactionButton(
            ButtonStyle.Danger,
            createLinkedCustomIdString(UNLINK_BUTTON_ID_PREFIX, user, uuid),
        ) {
            emoji(ReactionEmoji.Unicode("\uD83D\uDDD1\uFE0F"))
            label = "解除"
        }
    }
}
