package io.github.valine3gdev.mcguildlink.app.service.dto

import kotlin.time.Instant
import kotlin.uuid.Uuid


/**
 * 紐付けコード発行処理の結果を表します。
 */
sealed interface LinkRequestResult {
    /**
     * 紐付けコードを正常に発行または再利用できた結果です。
     */
    data class Success(val code: String) : LinkRequestResult

    /**
     * 対象アカウントがブロックされているためコードを発行できない結果です。
     */
    data object Blocked : LinkRequestResult
}


/**
 * 紐付け確定処理の結果を表します。
 */
sealed interface LinkResult {
    /**
     * Discord と Minecraft の紐付けが成功した結果です。
     */
    data class Success(val discordAccount: DiscordAccountInfo) : LinkResult

    /**
     * 指定コードが存在しない結果です。
     */
    data object InvalidCode : LinkResult

    /**
     * 指定された組み合わせがすでに紐付け済みである結果です。
     */
    data object AlreadyLinked : LinkResult

    /**
     * Discord か Minecraft のどちらかがブロック済みである結果です。
     */
    data object Blocked : LinkResult
}


/**
 * ブロック追加処理の結果を表します。
 */
sealed interface BlockResult {
    /**
     * 関連アカウント群のブロックが成功した結果です。
     */
    data class Success(val blockGroup: BlockedAccountGroupInfo) : BlockResult
    /**
     * 対象アカウント群がすでにブロック済みである結果です。
     */
    data object AlreadyBlocked : BlockResult
}


/**
 * ブロック解除処理の結果を表します。
 */
sealed interface UnblockResult {
    /**
     * ブロックグループ全体の解除に成功した結果です。
     */
    data class Success(val blockGroup: BlockedAccountGroupInfo) : UnblockResult

    /**
     * 対象アカウントがブロックされていなかった結果です。
     */
    data object NotBlocked : UnblockResult
}


/**
 * ブロックグループの表示用情報です。
 */
data class BlockedAccountGroupInfo(
    val rootDiscordAccount: DiscordAccountInfo,
    val blockedDiscordAccountInfos: List<DiscordAccountInfo>,
    val blockedMinecraftAccountInfos: List<MinecraftAccountInfo>,
    val createdAt: Instant,
) {
    val blockedDiscordAccounts: Int
        get() = blockedDiscordAccountInfos.size

    val blockedMinecraftAccounts: Int
        get() = blockedMinecraftAccountInfos.size
}


/**
 * Discord と Minecraft の紐付け 1 件を表す概要情報です。
 */
data class AccountLinkSummary(
    val discordAccount: DiscordAccountInfo,
    val minecraftAccount: MinecraftAccountInfo,
    val linkedAt: Instant,
)


/**
 * Discord アカウントの表示用情報です。
 */
data class DiscordAccountInfo(
    val userId: ULong,
    val lastKnownUsername: String,
) {
    override fun toString() = "$lastKnownUsername (`$userId`)"
}


/**
 * Minecraft アカウントの表示用情報です。
 */
data class MinecraftAccountInfo(
    val uuid: Uuid,
    val lastKnownName: String,
) {
    override fun toString() = "$lastKnownName (`$uuid`)"
}

/**
 * Minecraft アカウントのヘッド画像 URL を返します。
 */
fun MinecraftAccountInfo.toHeadAvatarUrl() = "https://mc-heads.net/avatar/$uuid"
