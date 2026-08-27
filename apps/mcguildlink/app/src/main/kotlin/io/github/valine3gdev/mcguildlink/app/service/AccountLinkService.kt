package io.github.valine3gdev.mcguildlink.app.service

import io.github.valine3gdev.mcguildlink.app.db.AccountLinkEntity
import io.github.valine3gdev.mcguildlink.app.db.LinkRequestEntity
import io.github.valine3gdev.mcguildlink.app.db.LinkRequests
import io.github.valine3gdev.mcguildlink.app.service.dto.*
import io.github.valine3gdev.mcguildlink.app.util.LinkCodeGenerator
import io.github.valine3gdev.mcguildlink.app.util.RandomLinkCodeGenerator
import org.jetbrains.exposed.v1.core.eq
import org.jetbrains.exposed.v1.jdbc.Database
import org.jetbrains.exposed.v1.jdbc.transactions.suspendTransaction
import org.jetbrains.exposed.v1.jdbc.transactions.transaction
import kotlin.time.Clock
import kotlin.uuid.Uuid


/**
 * Discord アカウントと Minecraft アカウントの紐付け管理を担当するサービスです。
 */
class AccountLinkService(
    private val db: Database,
    private val codeGenerator: LinkCodeGenerator = RandomLinkCodeGenerator(),
    private val whitelistRefreshRequester: WhitelistRefreshRequester = NoopWhitelistRefreshRequester,
) {
    /**
     * Discord ユーザーの紐付けコードを取得し、未発行なら新しく作成します。
     */
    suspend fun getOrCreateLinkRequest(discordUserId: ULong, username: String) = suspendTransaction(db) {
        val discord = AccountStore.getOrCreateDiscordAccount(discordUserId, username)
        if (discord.isBlocked()) {
            return@suspendTransaction LinkRequestResult.Blocked
        }

        val request = discord.linkRequest ?: LinkRequestEntity.new {
            discordAccount = discord
            code = generateUniqueCode()
        }

        LinkRequestResult.Success(request.code)
    }

    /**
     * 紐付けコードを消費して Discord アカウントと Minecraft アカウントを紐付けます。
     */
    fun consumeCodeAndLink(code: String, uuid: Uuid, name: String): LinkResult {
        val result = transaction(db) {
            val request = LinkRequestEntity.find {
                LinkRequests.code eq code
            }.firstOrNull() ?: return@transaction LinkResult.InvalidCode

            val discord = request.discordAccount
            if (discord.isBlocked()) {
                return@transaction LinkResult.Blocked
            }

            val minecraft = AccountStore.getOrCreateMinecraftAccount(uuid, name)
            if (minecraft.isBlocked()) {
                return@transaction LinkResult.Blocked
            }

            AccountStore.getAccountLinkOrNull(discord, minecraft)
                ?.let {
                    return@transaction LinkResult.AlreadyLinked
                }
                ?: AccountLinkEntity.new {
                    discordAccount = discord
                    minecraftAccount = minecraft
                    linkedAt = Clock.System.now()
                }

            request.delete()

            LinkResult.Success(DiscordAccountInfo(discord.userId, discord.lastKnownUsername))
        }

        if (result is LinkResult.Success) {
            whitelistRefreshRequester.requestRefresh()
        }

        return result
    }

    /**
     * Discord ユーザーに紐付いている Minecraft アカウント一覧を取得します。
     */
    suspend fun getLinkedMinecraftAccounts(userId: ULong): List<MinecraftAccountInfo> = suspendTransaction(db) {
        val discord = AccountStore.getDiscordAccountOrNull(userId) ?: return@suspendTransaction emptyList()
        discord.links.map { link ->
            val mc = link.minecraftAccount
            MinecraftAccountInfo(mc.uuid, mc.lastKnownName)
        }
    }

    /**
     * Minecraft アカウントに紐付いている Discord アカウント一覧を取得します。
     */
    suspend fun getLinkedDiscordAccounts(uuid: Uuid): List<DiscordAccountInfo> = suspendTransaction(db) {
        val minecraft = AccountStore.getMinecraftAccountOrNull(uuid) ?: return@suspendTransaction emptyList()
        minecraft.links.map { link ->
            val dc = link.discordAccount
            DiscordAccountInfo(dc.userId, dc.lastKnownUsername)
        }
    }

    /**
     * 指定した Discord ユーザーと Minecraft UUID の紐付け情報を取得します。
     */
    suspend fun getLinkOrNull(discordUserId: ULong, minecraftUuid: Uuid) = suspendTransaction(db) {
        val link = AccountStore.getAccountLinkOrNull(discordUserId, minecraftUuid) ?: return@suspendTransaction null
        val discord = link.discordAccount
        val minecraft = link.minecraftAccount
        Pair(
            DiscordAccountInfo(discord.userId, discord.lastKnownUsername),
            MinecraftAccountInfo(minecraft.uuid, minecraft.lastKnownName)
        )
    }

    /**
     * Discord ユーザーに紐付いたアカウント一覧を紐付け日時の降順で返します。
     */
    suspend fun listLinksByDiscord(discordUserId: ULong): List<AccountLinkSummary> = suspendTransaction(db) {
        val discord = AccountStore.getDiscordAccountOrNull(discordUserId) ?: return@suspendTransaction emptyList()
        discord.links
            .map { it.toSummary() }
            .sortedByDescending { it.linkedAt }
    }

    /**
     * Minecraft アカウントに紐付いたアカウント一覧を紐付け日時の降順で返します。
     */
    suspend fun listLinksByMinecraft(uuid: Uuid): List<AccountLinkSummary> = suspendTransaction(db) {
        val minecraft = AccountStore.getMinecraftAccountOrNull(uuid) ?: return@suspendTransaction emptyList()
        minecraft.links
            .map { it.toSummary() }
            .sortedByDescending { it.linkedAt }
    }

    /**
     * すべての紐付けを紐付け日時の降順で返します。
     */
    suspend fun listAllLinks(): List<AccountLinkSummary> = suspendTransaction(db) {
        AccountLinkEntity.all()
            .map { it.toSummary() }
            .sortedByDescending { it.linkedAt }
    }

    /**
     * 指定した組み合わせの紐付けを 1 件だけ解除します。
     */
    suspend fun unlink(discordUserId: ULong, minecraftUuid: Uuid): Boolean {
        val removed = suspendTransaction(db) {
            val link =
                AccountStore.getAccountLinkOrNull(discordUserId, minecraftUuid) ?: return@suspendTransaction false
            link.delete()
            true
        }

        if (removed) {
            whitelistRefreshRequester.requestRefresh()
        }

        return removed
    }

    /**
     * Discord ユーザーに紐付くすべての Minecraft アカウントを解除し、解除した一覧を返します。
     */
    suspend fun unlinkByDiscord(discordUserId: ULong): List<MinecraftAccountInfo> = suspendTransaction(db) {
        val discord = AccountStore.getDiscordAccountOrNull(discordUserId) ?: return@suspendTransaction null
        val links = discord.links.toList()
        if (links.isEmpty()) {
            return@suspendTransaction null
        }

        buildList<MinecraftAccountInfo>(links.size) {
            links.forEach { link ->
                val mc = link.minecraftAccount
                add(MinecraftAccountInfo(mc.uuid, mc.lastKnownName))
                link.delete()
            }
        }
    }?.also {
        whitelistRefreshRequester.requestRefresh()
    } ?: emptyList()

    /**
     * Discord ユーザーに紐付いた紐付けリクエストを削除します。
     */
    suspend fun deleteLinkRequestByDiscord(discordUserId: ULong): Boolean = suspendTransaction(db) {
        val discord = AccountStore.getDiscordAccountOrNull(discordUserId) ?: return@suspendTransaction false
        val request = discord.linkRequest ?: return@suspendTransaction false
        request.delete()
        true
    }

    /**
     * 既存コードと衝突しない紐付けコードを生成します。
     */
    private fun generateUniqueCode(): String {
        repeat(20) {
            val code = codeGenerator.generate()
            val isUnique = LinkRequestEntity.find {
                LinkRequests.code eq code
            }.empty()
            if (isUnique) return code
        }
        error("Failed to generate unique link code")
    }
}

/**
 * 永続化済みの紐付けエンティティを表示用 DTO に変換します。
 */
private fun AccountLinkEntity.toSummary(): AccountLinkSummary {
    val discord = discordAccount
    val minecraft = minecraftAccount

    return AccountLinkSummary(
        discordAccount = DiscordAccountInfo(discord.userId, discord.lastKnownUsername),
        minecraftAccount = MinecraftAccountInfo(minecraft.uuid, minecraft.lastKnownName),
        linkedAt = linkedAt,
    )
}
