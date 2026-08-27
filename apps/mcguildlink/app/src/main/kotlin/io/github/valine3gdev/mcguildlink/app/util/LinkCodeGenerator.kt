package io.github.valine3gdev.mcguildlink.app.util

import java.security.SecureRandom
import java.util.Random


/**
 * 紐付けコード生成の抽象化です。
 */
interface LinkCodeGenerator {
    /**
     * 新しい紐付けコードを 1 つ生成します。
     */
    fun generate(): String
}


/**
 * ランダムな文字列で紐付けコードを生成する実装です。
 */
class RandomLinkCodeGenerator(
    private val length: Int = 8,
    private val random: Random = SecureRandom(),
) : LinkCodeGenerator {
    init {
        require(length > 0) { "Code length must be positive" }
    }

    companion object {
        const val CHARS = "ACDEFGHJKMNPQRTUVWXYZacdefghjkmnpqrtuvwxyz234679"
    }

    /**
     * 設定された長さのランダムコードを生成します。
     */
    override fun generate(): String {
        return CharArray(length) {
            CHARS[random.nextInt(CHARS.length)]
        }.concatToString()
    }
}
