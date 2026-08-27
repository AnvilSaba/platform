package io.github.valine3gdev.mcguildlink.app.db

import org.jetbrains.exposed.v1.core.dao.id.EntityID
import org.jetbrains.exposed.v1.dao.IntEntity
import org.jetbrains.exposed.v1.dao.IntEntityClass


/**
 * Discord アカウントを表す Exposed エンティティです。
 */
class DiscordAccountEntity(id: EntityID<Int>) : IntEntity(id) {
    companion object : IntEntityClass<DiscordAccountEntity>(DiscordAccounts)

    var userId by DiscordAccounts.userId
    var lastKnownUsername by DiscordAccounts.lastKnownUsername

    val links by AccountLinkEntity referrersOn AccountLinks.discordAccount
    val linkRequest by LinkRequestEntity optionalBackReferencedOn LinkRequests.discordAccount
    val blockedMembership by BlockedDiscordAccountEntity optionalBackReferencedOn BlockedDiscordAccounts.discordAccount
}


/**
 * Minecraft アカウントを表す Exposed エンティティです。
 */
class MinecraftAccountEntity(id: EntityID<Int>) : IntEntity(id) {
    companion object : IntEntityClass<MinecraftAccountEntity>(MinecraftAccounts)

    var uuid by MinecraftAccounts.uuid
    var lastKnownName by MinecraftAccounts.lastKnownName

    val links by AccountLinkEntity referrersOn AccountLinks.minecraftAccount
    val blockedMembership by BlockedMinecraftAccountEntity optionalBackReferencedOn BlockedMinecraftAccounts.minecraftAccount
}


/**
 * Discord アカウントと Minecraft アカウントの紐付けを表す Exposed エンティティです。
 */
class AccountLinkEntity(id: EntityID<Int>) : IntEntity(id) {
    companion object : IntEntityClass<AccountLinkEntity>(AccountLinks)

    var discordAccount by DiscordAccountEntity referencedOn AccountLinks.discordAccount
    var minecraftAccount by MinecraftAccountEntity referencedOn AccountLinks.minecraftAccount
    var linkedAt by AccountLinks.linkedAt
}


/**
 * Discord アカウントに対して発行した紐付けコードを表す Exposed エンティティです。
 */
class LinkRequestEntity(id: EntityID<Int>) : IntEntity(id) {
    companion object : IntEntityClass<LinkRequestEntity>(LinkRequests)

    var discordAccount by DiscordAccountEntity referencedOn LinkRequests.discordAccount
    var code by LinkRequests.code
}


/**
 * 関連するブロック済みアカウント群をまとめる Exposed エンティティです。
 */
class BlockGroupEntity(id: EntityID<Int>) : IntEntity(id) {
    companion object : IntEntityClass<BlockGroupEntity>(BlockGroups)

    var rootDiscordAccount by DiscordAccountEntity referencedOn BlockGroups.rootDiscordAccount
    var createdAt by BlockGroups.createdAt

    val blockedDiscordAccounts by BlockedDiscordAccountEntity referrersOn BlockedDiscordAccounts.blockGroup
    val blockedMinecraftAccounts by BlockedMinecraftAccountEntity referrersOn BlockedMinecraftAccounts.blockGroup
}


/**
 * ブロックされた Discord アカウントを表す Exposed エンティティです。
 */
class BlockedDiscordAccountEntity(id: EntityID<Int>) : IntEntity(id) {
    companion object : IntEntityClass<BlockedDiscordAccountEntity>(BlockedDiscordAccounts)

    var blockGroup by BlockGroupEntity referencedOn BlockedDiscordAccounts.blockGroup
    var discordAccount by DiscordAccountEntity referencedOn BlockedDiscordAccounts.discordAccount
}


/**
 * ブロックされた Minecraft アカウントを表す Exposed エンティティです。
 */
class BlockedMinecraftAccountEntity(id: EntityID<Int>) : IntEntity(id) {
    companion object : IntEntityClass<BlockedMinecraftAccountEntity>(BlockedMinecraftAccounts)

    var blockGroup by BlockGroupEntity referencedOn BlockedMinecraftAccounts.blockGroup
    var minecraftAccount by MinecraftAccountEntity referencedOn BlockedMinecraftAccounts.minecraftAccount
}
