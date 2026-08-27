package io.github.valine3gdev.mcguildlink.app.service

import io.github.valine3gdev.mcguildlink.app.db.AccountLinkEntity
import io.github.valine3gdev.mcguildlink.app.db.AccountLinks
import io.github.valine3gdev.mcguildlink.app.db.BlockGroupEntity
import io.github.valine3gdev.mcguildlink.app.db.BlockedDiscordAccountEntity
import io.github.valine3gdev.mcguildlink.app.db.BlockedMinecraftAccountEntity
import io.github.valine3gdev.mcguildlink.app.db.DiscordAccountEntity
import io.github.valine3gdev.mcguildlink.app.db.LinkRequestEntity
import io.github.valine3gdev.mcguildlink.app.db.LinkRequests
import io.github.valine3gdev.mcguildlink.app.db.MinecraftAccountEntity
import io.github.valine3gdev.mcguildlink.app.service.dto.BlockedAccountGroupInfo
import io.github.valine3gdev.mcguildlink.app.service.dto.BlockResult
import io.github.valine3gdev.mcguildlink.app.service.dto.DiscordAccountInfo
import io.github.valine3gdev.mcguildlink.app.service.dto.MinecraftAccountInfo
import io.github.valine3gdev.mcguildlink.app.service.dto.UnblockResult
import org.jetbrains.exposed.v1.core.inList
import org.jetbrains.exposed.v1.core.or
import org.jetbrains.exposed.v1.jdbc.Database
import org.jetbrains.exposed.v1.jdbc.transactions.suspendTransaction
import kotlin.time.Clock
import kotlin.time.Instant


/**
 * Discord アカウントを起点に関連アカウント群のブロックと解除を行うサービスです。
 */
class AccountBlockService(
    private val db: Database,
    private val whitelistRefreshRequester: WhitelistRefreshRequester = NoopWhitelistRefreshRequester,
) {
    /**
     * Discord アカウントエンティティを表示用 DTO に変換します。
     */
    private fun DiscordAccountEntity.toInfo() = DiscordAccountInfo(userId, lastKnownUsername)

    /**
     * Minecraft アカウントエンティティを表示用 DTO に変換します。
     */
    private fun MinecraftAccountEntity.toInfo() = MinecraftAccountInfo(uuid, lastKnownName)

    /**
     * Discord アカウント群を root 先頭・以降 userId 昇順の表示用 DTO に変換します。
     */
    private fun Iterable<DiscordAccountEntity>.toBlockedDiscordAccountInfos(rootDiscord: DiscordAccountEntity): List<DiscordAccountInfo> =
        buildList {
            add(rootDiscord.toInfo())
            this@toBlockedDiscordAccountInfos.asSequence()
                .filter { it.id != rootDiscord.id }
                .sortedBy { it.userId }
                .mapTo(this) { it.toInfo() }
        }

    /**
     * Minecraft アカウント群を表示順つき DTO に変換します。
     */
    private fun Iterable<MinecraftAccountEntity>.toBlockedMinecraftAccountInfos(): List<MinecraftAccountInfo> =
        asSequence()
            .map { it.toInfo() }
            .sortedWith(
                compareBy(
                    { it.lastKnownName },
                    { it.uuid.toString() },
                )
            )
            .toList()

    /**
     * ブロックグループの表示用 DTO を組み立てます。
     */
    private fun blockedAccountGroupInfo(
        rootDiscord: DiscordAccountEntity,
        blockedDiscordAccounts: Iterable<DiscordAccountEntity>,
        blockedMinecraftAccounts: Iterable<MinecraftAccountEntity>,
        createdAt: Instant,
    ): BlockedAccountGroupInfo = BlockedAccountGroupInfo(
        rootDiscordAccount = rootDiscord.toInfo(),
        blockedDiscordAccountInfos = blockedDiscordAccounts.toBlockedDiscordAccountInfos(rootDiscord),
        blockedMinecraftAccountInfos = blockedMinecraftAccounts.toBlockedMinecraftAccountInfos(),
        createdAt = createdAt,
    )

    /**
     * ブロックグループエンティティを表示用 DTO に変換します。
     */
    private fun BlockGroupEntity.toInfo() = blockedAccountGroupInfo(
        rootDiscord = rootDiscordAccount,
        blockedDiscordAccounts = blockedDiscordAccounts.map { it.discordAccount },
        blockedMinecraftAccounts = blockedMinecraftAccounts.map { it.minecraftAccount },
        createdAt = createdAt,
    )

    /**
     * 指定した Discord アカウントとリンクでつながる Discord/Minecraft アカウント集合を収集します。
     */
    private fun collectConnectedAccounts(
        rootDiscord: DiscordAccountEntity
    ): Pair<Set<DiscordAccountEntity>, Set<MinecraftAccountEntity>> {
        val discordAccounts = linkedSetOf(rootDiscord)
        val minecraftAccounts = linkedSetOf<MinecraftAccountEntity>()

        while (true) {
            var changed = false

            if (discordAccounts.isNotEmpty()) {
                AccountLinkEntity.find {
                    AccountLinks.discordAccount inList discordAccounts.map { it.id }
                }.forEach { link ->
                    changed = minecraftAccounts.add(link.minecraftAccount) || changed
                }
            }

            if (minecraftAccounts.isNotEmpty()) {
                AccountLinkEntity.find {
                    AccountLinks.minecraftAccount inList minecraftAccounts.map { it.id }
                }.forEach { link ->
                    changed = discordAccounts.add(link.discordAccount) || changed
                }
            }

            if (!changed) {
                return discordAccounts to minecraftAccounts
            }
        }
    }

    /**
     * 指定した Discord ユーザーを起点に関連アカウント群をブロックし、既存リンクと発行済みコードを破棄します。
     */
    suspend fun blockDiscordAccount(discordUserId: ULong, username: String): BlockResult {
        val result = suspendTransaction(db) {
            val rootDiscord = AccountStore.getOrCreateDiscordAccount(discordUserId, username)
            if (rootDiscord.isBlocked()) {
                return@suspendTransaction BlockResult.AlreadyBlocked
            }

            val (discordAccounts, minecraftAccounts) = collectConnectedAccounts(rootDiscord)
            if (discordAccounts.any { it.isBlocked() } || minecraftAccounts.any { it.isBlocked() }) {
                return@suspendTransaction BlockResult.AlreadyBlocked
            }

            val blockGroup = BlockGroupEntity.new {
                rootDiscordAccount = rootDiscord
                createdAt = Clock.System.now()
            }

            discordAccounts.forEach { discord ->
                BlockedDiscordAccountEntity.new {
                    this.blockGroup = blockGroup
                    discordAccount = discord
                }
            }

            minecraftAccounts.forEach { minecraft ->
                BlockedMinecraftAccountEntity.new {
                    this.blockGroup = blockGroup
                    minecraftAccount = minecraft
                }
            }

            val discordIds = discordAccounts.mapTo(mutableListOf()) { it.id }
            val minecraftIds = minecraftAccounts.mapTo(mutableListOf()) { it.id }

            if (discordIds.isNotEmpty() || minecraftIds.isNotEmpty()) {
                AccountLinkEntity.find {
                    when {
                        discordIds.isNotEmpty() && minecraftIds.isNotEmpty() ->
                            (AccountLinks.discordAccount inList discordIds) or (AccountLinks.minecraftAccount inList minecraftIds)

                        discordIds.isNotEmpty() ->
                            AccountLinks.discordAccount inList discordIds

                        else ->
                            AccountLinks.minecraftAccount inList minecraftIds
                    }
                }.toList().forEach { it.delete() }
            }

            if (discordIds.isNotEmpty()) {
                LinkRequestEntity.find {
                    LinkRequests.discordAccount inList discordIds
                }.toList().forEach { it.delete() }
            }

            BlockResult.Success(
                blockedAccountGroupInfo(
                    rootDiscord = rootDiscord,
                    blockedDiscordAccounts = discordAccounts,
                    blockedMinecraftAccounts = minecraftAccounts,
                    createdAt = blockGroup.createdAt,
                )
            )
        }

        if (result is BlockResult.Success) {
            whitelistRefreshRequester.requestRefresh()
        }

        return result
    }

    /**
     * 指定した Discord ユーザーが属するブロックグループ全体を解除します。
     */
    suspend fun unblockDiscordAccount(discordUserId: ULong): UnblockResult {
        val result = suspendTransaction(db) {
            val discord = AccountStore.getDiscordAccountOrNull(discordUserId)
                ?: return@suspendTransaction UnblockResult.NotBlocked
            val blockGroup = discord.blockedMembership?.blockGroup ?: return@suspendTransaction UnblockResult.NotBlocked
            val blockGroupInfo = blockGroup.toInfo()

            blockGroup.blockedDiscordAccounts.toList().forEach { it.delete() }
            blockGroup.blockedMinecraftAccounts.toList().forEach { it.delete() }
            blockGroup.delete()

            UnblockResult.Success(blockGroupInfo)
        }

        return result
    }

    /**
     * 登録済みのブロックグループ一覧を作成日時の降順で返します。
     */
    suspend fun listBlockedDiscordAccountGroups() = suspendTransaction(db) {
        BlockGroupEntity.all()
            .sortedByDescending { it.createdAt }
            .map { it.toInfo() }
    }
}
