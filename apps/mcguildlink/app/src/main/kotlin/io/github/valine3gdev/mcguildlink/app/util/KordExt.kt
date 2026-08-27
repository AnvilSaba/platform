package io.github.valine3gdev.mcguildlink.app.util

import dev.kord.core.entity.User
import io.github.valine3gdev.mcguildlink.app.service.AccountBlockService
import io.github.valine3gdev.mcguildlink.app.service.AccountLinkService
import kotlin.uuid.Uuid


/**
 * Discord ユーザー情報を使って紐付けコードを取得または発行します。
 */
suspend fun AccountLinkService.getOrCreateLinkRequest(user: User) =
    getOrCreateLinkRequest(user.id.value, user.username)

/**
 * Discord ユーザーに紐付いた Minecraft アカウント一覧を取得します。
 */
suspend fun AccountLinkService.getLinkedMinecraftAccounts(user: User) =
    getLinkedMinecraftAccounts(user.id.value)

/**
 * Discord ユーザーと Minecraft UUID の紐付け情報を取得します。
 */
suspend fun AccountLinkService.getLinkOrNull(user: User, minecraftUuid: Uuid) =
    getLinkOrNull(user.id.value, minecraftUuid)

/**
 * Discord ユーザーと指定 Minecraft UUID の紐付けを解除します。
 */
suspend fun AccountLinkService.unlink(user: User, minecraftUuid: Uuid) =
    unlink(user.id.value, minecraftUuid)

/**
 * Discord ユーザーに紐付く Minecraft アカウントをすべて解除します。
 */
suspend fun AccountLinkService.unlinkByDiscord(user: User) =
    unlinkByDiscord(user.id.value)

/**
 * Discord ユーザーに紐付いた紐付けリクエストを削除します。
 */
suspend fun AccountLinkService.deleteLinkRequestByDiscord(user: User) =
    deleteLinkRequestByDiscord(user.id.value)

/**
 * Discord ユーザーを起点に関連アカウント群をブロックします。
 */
suspend fun AccountBlockService.blockDiscordAccount(user: User) =
    blockDiscordAccount(user.id.value, user.username)

/**
 * Discord ユーザーが属するブロックグループを解除します。
 */
suspend fun AccountBlockService.unblockDiscordAccount(user: User) =
    unblockDiscordAccount(user.id.value)
