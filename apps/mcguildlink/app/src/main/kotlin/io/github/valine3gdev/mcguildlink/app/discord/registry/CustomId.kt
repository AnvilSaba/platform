package io.github.valine3gdev.mcguildlink.app.discord.registry

import dev.kord.common.entity.Snowflake
import dev.kord.core.entity.User
import kotlin.uuid.Uuid


/**
 * Discord コンポーネントの custom ID マッチ条件を表します。
 */
sealed interface CustomId {
    /**
     * 指定 ID がこの条件に一致するか判定します。
     */
    fun matches(id: String): Boolean
}

/**
 * custom ID 文字列から追加データを復元する抽象化です。
 */
interface CustomIdParser<T> {
    /**
     * 指定 ID から追加データを取り出します。
     */
    fun parse(id: String): T?
}

/**
 * 完全一致で custom ID を判定する実装です。
 */
class EqualsCustomId(private val value: String) : CustomId {
    /**
     * custom ID が保持値と一致する場合に `true` を返します。
     */
    override fun matches(id: String): Boolean = value == id
}

/**
 * 接頭辞一致で custom ID を判定し、必要に応じて追加データも復元する実装です。
 */
class PrefixCustomId<T>(
    private val prefix: String,
    private val parser: ((String) -> T)? = null
) : CustomId, CustomIdParser<T> {
    /**
     * custom ID が接頭辞で始まる場合に `true` を返します。
     */
    override fun matches(id: String): Boolean = id.startsWith(prefix)

    /**
     * 接頭辞を取り除いた残り文字列から追加データを復元します。
     */
    override fun parse(id: String): T? = parser?.invoke(id.removePrefix(prefix))
}

/**
 * `ユーザー ID / Minecraft UUID` 形式の custom ID を扱うパーサー付き ID 定義を作成します。
 */
fun createLinkedCustomId(prefix: String) = PrefixCustomId(prefix) { data ->
    data.split("/").takeIf { it.size == 2 }?.let {
        val (userIdStr, uuidStr) = it
        val uuid = Uuid.parseHexDash(uuidStr)
        val userId = Snowflake(userIdStr.toULong())
        userId to uuid
    }
}

/**
 * `ユーザー ID / Minecraft UUID` を含む custom ID 文字列を生成します。
 */
fun createLinkedCustomIdString(prefix: String, user: User, mcUuid: Uuid) =
    "${prefix}${user.id.value}/${mcUuid.toHexDashString()}"
