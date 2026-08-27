package io.github.valine3gdev.mcguildlink.app.service

import io.github.valine3gdev.mcguildlink.app.db.AccountLinkEntity
import io.github.valine3gdev.mcguildlink.app.db.AccountLinks
import io.github.valine3gdev.mcguildlink.app.db.DiscordAccountEntity
import io.github.valine3gdev.mcguildlink.app.db.DiscordAccounts
import io.github.valine3gdev.mcguildlink.app.db.MinecraftAccountEntity
import io.github.valine3gdev.mcguildlink.app.db.MinecraftAccounts
import org.jetbrains.exposed.v1.core.and
import org.jetbrains.exposed.v1.core.eq
import kotlin.uuid.Uuid


/**
 * アカウント関連エンティティの検索・生成をまとめた内部ヘルパーです。
 */
internal object AccountStore {
    /**
     * Discord ユーザー ID から対応する Discord アカウントを取得します。
     */
    fun getDiscordAccountOrNull(userId: ULong) = DiscordAccountEntity.find {
        DiscordAccounts.userId eq userId
    }.firstOrNull()

    /**
     * Minecraft UUID から対応する Minecraft アカウントを取得します。
     */
    fun getMinecraftAccountOrNull(uuid: Uuid) = MinecraftAccountEntity.find {
        MinecraftAccounts.uuid eq uuid
    }.firstOrNull()

    /**
     * Discord アカウントを取得し、存在しなければ新規作成します。既存の場合はユーザー名を更新します。
     */
    fun getOrCreateDiscordAccount(discordUserId: ULong, username: String) =
        getDiscordAccountOrNull(discordUserId)?.apply {
            lastKnownUsername = username
        } ?: DiscordAccountEntity.new {
            this.userId = discordUserId
            this.lastKnownUsername = username
        }

    /**
     * Minecraft アカウントを取得し、存在しなければ新規作成します。既存の場合は表示名を更新します。
     */
    fun getOrCreateMinecraftAccount(uuid: Uuid, name: String) =
        getMinecraftAccountOrNull(uuid)?.apply {
            lastKnownName = name
        } ?: MinecraftAccountEntity.new {
            this.uuid = uuid
            this.lastKnownName = name
        }

    /**
     * 2 つのエンティティ間の紐付けがあれば取得します。
     */
    fun getAccountLinkOrNull(discord: DiscordAccountEntity, minecraft: MinecraftAccountEntity) =
        AccountLinkEntity.find {
            (AccountLinks.discordAccount eq discord.id) and (AccountLinks.minecraftAccount eq minecraft.id)
        }.firstOrNull()

    /**
     * Discord ユーザー ID と Minecraft UUID の組み合わせから紐付けを取得します。
     */
    fun getAccountLinkOrNull(discordUserId: ULong, minecraftUuid: Uuid): AccountLinkEntity? {
        val discord = getDiscordAccountOrNull(discordUserId) ?: return null
        val minecraft = getMinecraftAccountOrNull(minecraftUuid) ?: return null
        return getAccountLinkOrNull(discord, minecraft)
    }
}


/**
 * Discord アカウントがブロック済みかどうかを判定します。
 */
internal fun DiscordAccountEntity.isBlocked() = blockedMembership != null

/**
 * Minecraft アカウントがブロック済みかどうかを判定します。
 */
internal fun MinecraftAccountEntity.isBlocked() = blockedMembership != null
