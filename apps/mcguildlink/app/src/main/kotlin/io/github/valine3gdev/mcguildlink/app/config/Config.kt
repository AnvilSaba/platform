package io.github.valine3gdev.mcguildlink.app.config

import dev.eav.tomlkt.Toml
import dev.kord.common.entity.Snowflake
import kotlinx.serialization.SerialName
import kotlinx.serialization.Serializable
import kotlinx.serialization.decodeFromString
import java.nio.file.Path
import kotlin.io.path.readText
import kotlin.time.Duration


/**
 * アプリケーション全体の TOML 設定を表します。
 */
@Serializable
data class Config(
    val bot: BotConfig,

    @SerialName("minecraft_server")
    val minecraftServer: MinecraftServerConfig,

    val web: WebConfig,
) {
    companion object {
        /**
         * 指定した TOML ファイルを読み込み、[Config] として復元します。
         */
        fun load(path: Path) = Toml.decodeFromString<Config>(path.readText())

    }
}


/**
 * Discord Bot の接続先と権限関連設定です。
 */
@Serializable
data class BotConfig(
    val token: String,
    val guild: Snowflake,

    @SerialName("moderator_role")
    val moderatorRole: Snowflake,

    @SerialName("log_channel")
    val logChannel: Snowflake,

    @SerialName("display_server_address")
    val displayServerAddress: String,
)


/**
 * Minestom ベースの Minecraft サーバー設定です。
 */
@Serializable
data class MinecraftServerConfig(
    val address: String,
    val port: Int,
    val timeout: Duration,
)


/**
 * ホワイトリスト配信用 Web サーバー設定です。
 */
@Serializable
data class WebConfig(
    val address: String,
    val port: Int,
)
