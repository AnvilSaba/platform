package io.github.valine3gdev.mcguildlink.app.discord.accountlink.interactions

import dev.kord.common.DiscordTimestampStyle
import dev.kord.common.entity.SeparatorSpacingSize
import dev.kord.common.toMessageFormat
import dev.kord.core.entity.interaction.ActionInteraction
import dev.kord.rest.builder.component.ContainerBuilder
import dev.kord.rest.builder.component.section
import dev.kord.rest.builder.component.separator
import io.github.valine3gdev.mcguildlink.app.discord.registry.EphemeralPagination
import io.github.valine3gdev.mcguildlink.app.discord.registry.InteractionRegistry
import io.github.valine3gdev.mcguildlink.app.discord.registry.PaginationSnapshotPage
import io.github.valine3gdev.mcguildlink.app.service.dto.AccountLinkSummary
import io.github.valine3gdev.mcguildlink.app.service.dto.toHeadAvatarUrl


private val discordAccountLinksPagination = createAccountLinksPagination(
    prefix = "list_links_discord_page_button:",
    title = "## 紐付け一覧 (Discord)",
    description = "指定した Discordアカウントに紐付けられている Minecraftアカウントを表示しています。",
)

private val minecraftAccountLinksPagination = createAccountLinksPagination(
    prefix = "list_links_minecraft_page_button:",
    title = "## 紐付け一覧 (Minecraft)",
    description = "指定した Minecraftアカウントに紐付けられている Discordアカウントを表示しています。",
)

private val allAccountLinksPagination = createAccountLinksPagination(
    prefix = "list_links_all_page_button:",
    title = "## 紐付け一覧 (All)",
    description = "現在の全ての紐付けを新しい順に表示しています。",
)

/**
 * 紐付け一覧用ページ送りボタンを登録します。
 */
internal fun InteractionRegistry.installAccountLinksPagination() {
    discordAccountLinksPagination.installPaginationButton()
    minecraftAccountLinksPagination.installPaginationButton()
    allAccountLinksPagination.installPaginationButton()
}

/**
 * Discord アカウント単位の紐付け一覧をエフェメラルページネーションで返信します。
 */
internal suspend fun respondDiscordAccountLinksPaginated(
    interaction: ActionInteraction,
    links: List<AccountLinkSummary>,
) {
    discordAccountLinksPagination.respondEphemeralPaginatedSnapshot(interaction, links)
}

/**
 * Minecraft アカウント単位の紐付け一覧をエフェメラルページネーションで返信します。
 */
internal suspend fun respondMinecraftAccountLinksPaginated(
    interaction: ActionInteraction,
    links: List<AccountLinkSummary>,
) {
    minecraftAccountLinksPagination.respondEphemeralPaginatedSnapshot(interaction, links)
}

/**
 * 全紐付け一覧をエフェメラルページネーションで返信します。
 */
internal suspend fun respondAllAccountLinksPaginated(
    interaction: ActionInteraction,
    links: List<AccountLinkSummary>,
) {
    allAccountLinksPagination.respondEphemeralPaginatedSnapshot(interaction, links)
}

/**
 * 共通レイアウトを使う紐付け一覧ページネーションを生成します。
 */
private fun createAccountLinksPagination(
    prefix: String,
    title: String,
    description: String,
) = EphemeralPagination(
    prefix = prefix,
    pageSize = 5,
    renderPage = { _, page -> applyAccountLinksPage(title, description, page) },
)

/**
 * 紐付け一覧 1 ページ分の本文を描画します。
 */
private fun ContainerBuilder.applyAccountLinksPage(
    title: String,
    description: String,
    page: PaginationSnapshotPage<AccountLinkSummary>,
) {
    textDisplay(
        """
            $title
            $description
        """.trimIndent()
    )

    separator {
        divider = false
    }

    page.entries.forEachIndexed { index, link ->
        if (index > 0) {
            separator(SeparatorSpacingSize.Large)
        }

        section {
            textDisplay(
                """
                    - Discord: ${link.discordAccount}
                    - Minecraft: ${link.minecraftAccount}
                    - 紐付け日時: ${link.linkedAt.toMessageFormat(DiscordTimestampStyle.LongDateTime)}
                """.trimIndent()
            )
            thumbnailAccessory {
                url = link.minecraftAccount.toHeadAvatarUrl()
            }
        }
    }
}
