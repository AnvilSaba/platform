package io.github.valine3gdev.mcguildlink.app.db

import org.jetbrains.exposed.v1.core.dao.id.IntIdTable
import org.jetbrains.exposed.v1.datetime.timestamp


/**
 * Discord アカウントの永続化テーブルです。
 */
object DiscordAccounts : IntIdTable("discord_accounts") {
    val userId = ulong("user_id").uniqueIndex()
    val lastKnownUsername = varchar("last_known_username", length = 32)
}


/**
 * Minecraft アカウントの永続化テーブルです。
 */
object MinecraftAccounts : IntIdTable("minecraft_accounts") {
    val uuid = uuid("uuid").uniqueIndex()
    val lastKnownName = varchar("last_known_name", length = 16)
}


/**
 * Discord アカウントと Minecraft アカウントの紐付けを保持するテーブルです。
 */
object AccountLinks : IntIdTable("account_links") {
    val discordAccount = reference("discord_account_id", DiscordAccounts)
    val minecraftAccount = reference("minecraft_account_id", MinecraftAccounts)
    val linkedAt = timestamp("linked_at")

    init {
        uniqueIndex(discordAccount, minecraftAccount)
    }
}


/**
 * Discord ユーザーへ発行した紐付けコードを保持するテーブルです。
 */
object LinkRequests : IntIdTable("link_requests") {
    val discordAccount = reference("discord_account_id", DiscordAccounts).uniqueIndex()
    val code = varchar("code", length = 64).uniqueIndex()
}


/**
 * 関連アカウントをまとめて管理するブロックグループのテーブルです。
 */
object BlockGroups : IntIdTable("block_groups") {
    val rootDiscordAccount = reference("root_discord_account_id", DiscordAccounts)
    val createdAt = timestamp("created_at")
}


/**
 * ブロック対象の Discord アカウントを保持するテーブルです。
 */
object BlockedDiscordAccounts : IntIdTable("blocked_discord_accounts") {
    val blockGroup = reference("block_group_id", BlockGroups)
    val discordAccount = reference("discord_account_id", DiscordAccounts).uniqueIndex()
}


/**
 * ブロック対象の Minecraft アカウントを保持するテーブルです。
 */
object BlockedMinecraftAccounts : IntIdTable("blocked_minecraft_accounts") {
    val blockGroup = reference("block_group_id", BlockGroups)
    val minecraftAccount = reference("minecraft_account_id", MinecraftAccounts).uniqueIndex()
}
