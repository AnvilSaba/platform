package io.github.valine3gdev.mcguildlink.app.discord.registry

import dev.kord.common.entity.ButtonStyle
import dev.kord.common.entity.MessageFlag
import dev.kord.common.entity.MessageFlags
import dev.kord.common.entity.SeparatorSpacingSize
import dev.kord.common.entity.Snowflake
import dev.kord.core.behavior.interaction.respondEphemeral
import dev.kord.core.behavior.interaction.updateEphemeralMessage
import dev.kord.core.entity.User
import dev.kord.core.entity.interaction.ActionInteraction
import dev.kord.core.event.interaction.ComponentInteractionCreateEvent
import dev.kord.rest.builder.component.ContainerBuilder
import dev.kord.rest.builder.component.actionRow
import dev.kord.rest.builder.message.MessageBuilder
import dev.kord.rest.builder.message.container
import java.util.concurrent.ConcurrentHashMap
import kotlin.time.Clock
import kotlin.time.Duration
import kotlin.time.Duration.Companion.minutes
import kotlin.time.Instant


/**
 * ページネーション用 custom ID から復元したページ要求情報です。
 */
data class PaginationCustomIdData(
    val userId: Snowflake,
    val snapshotId: String,
    val page: Int,
)


/**
 * ページネーション済みスナップショットの 1 ページ分を表します。
 */
data class PaginationSnapshotPage<T>(
    val snapshotId: String,
    val ownerUserId: Snowflake,
    val entries: List<T>,
    val currentPage: Int,
    val totalPages: Int,
    val totalEntries: Int,
    val expiresAt: Instant,
) {
    val hasPreviousPage: Boolean
        get() = currentPage > 0

    val hasNextPage: Boolean
        get() = currentPage + 1 < totalPages
}


/**
 * ページネーション対象一覧の一時スナップショットです。
 */
private data class PaginationSnapshot<T>(
    val id: String,
    val ownerUserId: Snowflake,
    val entries: List<T>,
    val pageSize: Int,
    val expiresAt: Instant,
) {
    /**
     * 要求ページ番号を正規化し、該当するページ情報を返します。
     */
    fun page(requestedPage: Int): PaginationSnapshotPage<T> {
        val totalEntries = entries.size
        val totalPages = if (totalEntries == 0) 1 else (totalEntries + pageSize - 1) / pageSize
        val currentPage = requestedPage.coerceIn(0, totalPages - 1)
        val fromIndex = currentPage * pageSize
        val toIndex = minOf(fromIndex + pageSize, totalEntries)

        return PaginationSnapshotPage(
            snapshotId = id,
            ownerUserId = ownerUserId,
            entries = entries.subList(fromIndex, toIndex),
            currentPage = currentPage,
            totalPages = totalPages,
            totalEntries = totalEntries,
            expiresAt = expiresAt,
        )
    }
}


/**
 * 一時的なページネーションスナップショットを保持するストアです。
 */
class PaginationSnapshotStore<T>(
    private val ttl: Duration = 10.minutes,
) {
    private val snapshots = ConcurrentHashMap<String, PaginationSnapshot<T>>()

    /**
     * 新しいスナップショットを登録し、先頭ページを返します。
     */
    fun create(snapshotId: String, ownerUserId: Snowflake, entries: List<T>, pageSize: Int): PaginationSnapshotPage<T> {
        require(pageSize > 0) { "pageSize must be positive" }

        pruneExpired()

        val snapshot = PaginationSnapshot(
            id = snapshotId,
            ownerUserId = ownerUserId,
            entries = entries.toList(),
            pageSize = pageSize,
            expiresAt = Clock.System.now() + ttl,
        )

        snapshots[snapshot.id] = snapshot
        return snapshot.page(0)
    }

    /**
     * スナップショット ID とページ番号からページ情報を取得します。
     */
    fun getPage(snapshotId: String, page: Int): PaginationSnapshotPage<T>? {
        pruneExpired()

        val snapshot = snapshots[snapshotId] ?: return null
        if (snapshot.expiresAt.isExpired()) {
            snapshots.remove(snapshotId, snapshot)
            return null
        }

        return snapshot.page(page)
    }

    /**
     * 指定したスナップショットを削除します。
     */
    fun remove(snapshotId: String) {
        snapshots.remove(snapshotId)
    }

    /**
     * 有効期限切れスナップショットをストアから除去します。
     */
    private fun pruneExpired(now: Instant = Clock.System.now()) {
        snapshots.entries.removeIf { it.value.expiresAt.isExpired(now) }
    }
}


/**
 * エフェメラルメッセージ向けのページネーション UI を構築するヘルパーです。
 */
class EphemeralPagination<T>(
    val prefix: String,
    val pageSize: Int,
    private val renderPage: ContainerBuilder.(User, PaginationSnapshotPage<T>) -> Unit,
    private val snapshotStore: PaginationSnapshotStore<T> = PaginationSnapshotStore(),
) {
    init {
        require(pageSize > 0) { "pageSize must be positive" }
    }

    private val paginationCustomId = createPaginationCustomId(prefix)

    /**
     * インタラクション単位のスナップショットを新規作成し、先頭ページを返します。
     */
    fun createSnapshot(interaction: ActionInteraction, entries: List<T>): PaginationSnapshotPage<T> =
        snapshotStore.create(
            snapshotId = interaction.id.value.toString(),
            ownerUserId = interaction.user.id,
            entries = entries,
            pageSize = pageSize,
        )

    /**
     * custom ID から復元した要求情報をもとに対象ページを取得します。
     */
    fun getPage(paginationData: PaginationCustomIdData): PaginationSnapshotPage<T>? =
        snapshotStore.getPage(paginationData.snapshotId, paginationData.page)

    /**
     * 指定ユーザー専用のページ遷移 custom ID を生成します。
     */
    private fun createCustomIdString(user: User, snapshotId: String, page: Int): String =
        "${prefix}${user.id.value}/$snapshotId/$page"

    /**
     * 前後ページ移動用のボタン群を描画します。
     */
    private fun ContainerBuilder.paginationButtons(
        user: User,
        page: PaginationSnapshotPage<T>,
    ) {
        if (page.totalPages <= 1) {
            return
        }

        separator(SeparatorSpacingSize.Large)

        actionRow {
            interactionButton(
                ButtonStyle.Primary,
                createCustomIdString(
                    user = user,
                    snapshotId = page.snapshotId,
                    page = (page.currentPage - 1).coerceAtLeast(0),
                ),
            ) {
                label = "◀"
                disabled = !page.hasPreviousPage
            }

            interactionButton(ButtonStyle.Secondary, "${prefix}disabled") {
                label = "${page.currentPage + 1} / ${page.totalPages} (${page.totalEntries})"
                disabled = true
            }

            interactionButton(
                ButtonStyle.Primary,
                createCustomIdString(
                    user = user,
                    snapshotId = page.snapshotId,
                    page = (page.currentPage + 1).coerceAtMost(page.totalPages - 1),
                ),
            ) {
                label = "▶"
                disabled = !page.hasNextPage
            }
        }
    }

    /**
     * ページ本体とページネーションボタンを含むメッセージを構築します。
     */
    private fun MessageBuilder.paginatedSnapshot(
        user: User,
        page: PaginationSnapshotPage<T>,
    ) {
        flags = MessageFlags(MessageFlag.IsComponentsV2)

        container {
            renderPage(user, page)
            paginationButtons(user, page)
        }
    }

    /**
     * 既存エフェメラルメッセージをページ内容で更新します。
     */
    private suspend fun updateEphemeral(
        event: ComponentInteractionCreateEvent,
        paginationData: PaginationCustomIdData,
        onInvalidUser: suspend ComponentInteractionCreateEvent.() -> Unit,
        onSnapshotMissing: MessageBuilder.() -> Unit,
    ) {
        if (paginationData.userId != event.interaction.user.id) {
            onInvalidUser(event)
            return
        }

        val page = getPage(paginationData)
        if (page == null || page.ownerUserId != event.interaction.user.id) {
            event.interaction.updateEphemeralMessage {
                onSnapshotMissing()
            }
            return
        }

        event.interaction.updateEphemeralMessage {
            paginatedSnapshot(event.interaction.user, page)
        }
    }

    context(registry: InteractionRegistry)
    /**
     * ページ送りボタンを [InteractionRegistry] に登録します。
     */
    fun installPaginationButton(
        onInvalidUser: suspend ComponentInteractionCreateEvent.() -> Unit = {
            interaction.respondEphemeral {
                content = "不正な操作です。このボタンはあなたのものではありません。"
            }
        },
        onSnapshotMissing: MessageBuilder.() -> Unit = {
            expiredPaginatedSnapshotMessage()
        },
    ) {
        registry.interactionButton(paginationCustomId) {
            val paginationData =
                it ?: error("Invalid pagination custom ID data for button: ${interaction.componentId}")

            updateEphemeral(this, paginationData, onInvalidUser, onSnapshotMissing)
        }
    }

    /**
     * 先頭ページを含むエフェメラルページネーションメッセージを返信します。
     */
    suspend fun respondEphemeralPaginatedSnapshot(
        interaction: ActionInteraction,
        entries: List<T>,
    ) {
        val firstPage = createSnapshot(interaction, entries)

        interaction.respondEphemeral {
            paginatedSnapshot(interaction.user, firstPage)
        }
    }
}


/**
 * 期限切れかどうかを判定します。
 */
private fun Instant.isExpired(now: Instant = Clock.System.now()): Boolean = this <= now


/**
 * `ユーザー ID / スナップショット ID / ページ番号` 形式のページネーション custom ID 定義を作成します。
 */
fun createPaginationCustomId(prefix: String) = PrefixCustomId(prefix) { data ->
    data.split("/").takeIf { it.size == 3 }?.let { (userIdStr, snapshotId, pageStr) ->
        val userId = userIdStr.toULongOrNull()?.let(::Snowflake) ?: return@let null
        val page = pageStr.toIntOrNull() ?: return@let null
        PaginationCustomIdData(userId, snapshotId, page)
    }
}

/**
 * スナップショット期限切れ時に表示する既定メッセージを構築します。
 */
fun MessageBuilder.expiredPaginatedSnapshotMessage(
    text: String = "一覧の有効期限が切れました。もう一度開き直してください。",
) {
    flags = MessageFlags(MessageFlag.IsComponentsV2)

    container {
        textDisplay(text)
    }
}
