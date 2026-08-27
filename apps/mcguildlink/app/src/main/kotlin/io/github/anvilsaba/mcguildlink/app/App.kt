package io.github.anvilsaba.mcguildlink.app

import dev.kord.core.Kord
import io.github.anvilsaba.mcguildlink.app.config.Config
import io.github.anvilsaba.mcguildlink.app.db.DatabaseFactory
import io.github.anvilsaba.mcguildlink.app.discord.Bot
import io.github.anvilsaba.mcguildlink.app.discord.logging.AuditLogSender
import io.github.anvilsaba.mcguildlink.app.discord.logging.DiscordAuditLogSender
import io.github.anvilsaba.mcguildlink.app.minecraft.MinecraftServer
import io.github.anvilsaba.mcguildlink.app.service.AccountBlockService
import io.github.anvilsaba.mcguildlink.app.service.AccountLinkService
import io.github.anvilsaba.mcguildlink.app.service.WhitelistFileSyncService
import io.github.anvilsaba.mcguildlink.app.web.configureWhitelistRouting
import io.ktor.client.HttpClient
import io.ktor.client.engine.java.Java
import io.ktor.server.engine.embeddedServer
import io.ktor.server.netty.Netty
import kotlinx.coroutines.coroutineScope
import kotlinx.coroutines.launch


/**
 * 設定読み込み、各種サービス初期化、Discord Bot・Minecraft サーバー・Web サーバーの起動をまとめる
 * アプリケーションエントリポイントです。
 */
class App(
    paths: AppPaths = AppPaths.detect()
) {
    private val config = Config.load(paths.configFile)

    private val db = DatabaseFactory.connect(paths.dbFile)

    private val whitelistFileSyncService = WhitelistFileSyncService(
        db = db,
        whitelistFile = paths.whitelistFile,
    )

    private val accountLinkService = AccountLinkService(
        db = db,
        whitelistRefreshRequester = whitelistFileSyncService,
    )
    private val accountBlockService = AccountBlockService(
        db = db,
        whitelistRefreshRequester = whitelistFileSyncService,
    )

    private val webServer = embeddedServer(Netty, host = config.web.address, port = config.web.port) {
        configureWhitelistRouting(paths.whitelistFile)
    }

    /**
     * ホワイトリスト同期、Discord Bot、Minecraft サーバー、Web サーバーを起動します。
     */
    suspend fun start() {
        whitelistFileSyncService.generateNow()
        val kord = Kord(config.bot.token) {
            httpClient = HttpClient(Java)
        }

        webServer.start(wait = false)

        try {
            coroutineScope {
                val auditLogSender: AuditLogSender = DiscordAuditLogSender(
                    kord = kord,
                    channelId = config.bot.logChannel,
                    scope = this,
                )
                val bot = Bot(
                    kord = kord,
                    config = config.bot,
                    accountLinkService = accountLinkService,
                    accountBlockService = accountBlockService,
                    auditLogSender = auditLogSender,
                )
                val minecraftServer = MinecraftServer(
                    config = config.minecraftServer,
                    accountLinkService = accountLinkService,
                    auditLogSender = auditLogSender,
                )

                whitelistFileSyncService.attach(this)

                minecraftServer.start()

                launch { bot.start() }
            }
        } finally {
            webServer.stop()
        }
    }
}

/**
 * デフォルト設定でアプリケーションを生成して起動します。
 */
suspend fun main() {
    val app = App()
    app.start()
}
