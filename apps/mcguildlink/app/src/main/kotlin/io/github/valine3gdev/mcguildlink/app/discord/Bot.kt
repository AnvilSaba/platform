package io.github.valine3gdev.mcguildlink.app.discord

import dev.kord.core.Kord
import dev.kord.core.behavior.requestMembers
import dev.kord.core.event.gateway.ReadyEvent
import dev.kord.core.event.guild.GuildCreateEvent
import dev.kord.core.on
import dev.kord.gateway.Intent
import dev.kord.gateway.PrivilegedIntent
import io.github.oshai.kotlinlogging.KotlinLogging
import io.github.valine3gdev.mcguildlink.app.config.BotConfig
import io.github.valine3gdev.mcguildlink.app.discord.accountlink.installAccountLinkHandlers
import io.github.valine3gdev.mcguildlink.app.discord.accountlink.installCommands
import io.github.valine3gdev.mcguildlink.app.discord.logging.AuditLogSender
import io.github.valine3gdev.mcguildlink.app.discord.registry.InteractionRegistry
import io.github.valine3gdev.mcguildlink.app.service.AccountBlockService
import io.github.valine3gdev.mcguildlink.app.service.AccountLinkService
import kotlinx.coroutines.flow.collect


private val logger = KotlinLogging.logger {}

/**
 * Discord Bot のイベント購読、コマンド登録、ログイン処理をまとめる起動クラスです。
 */
class Bot(
    private val kord: Kord,
    private val config: BotConfig,
    private val accountLinkService: AccountLinkService,
    private val accountBlockService: AccountBlockService,
    private val auditLogSender: AuditLogSender,
) {
    /**
     * インタラクションハンドラを登録し、必要な Gateway Intent を有効化して Bot を起動します。
     */
    suspend fun start() {
        val interactions = InteractionRegistry(kord)
        context(config, accountLinkService, accountBlockService) {
            context(auditLogSender) {
                installAccountLinkHandlers(kord, interactions)
            }

            installCommands(kord)
        }

        kord.on<GuildCreateEvent> {
            @OptIn(PrivilegedIntent::class)
            guild.requestMembers().collect()
        }

        kord.on<ReadyEvent> {
            logger.info { "Bot ready: ${self.username} (${kord.selfId})" }
        }

        kord.login {
            intents {
                @OptIn(PrivilegedIntent::class)
                +Intent.GuildMembers

                +Intent.Guilds
                +Intent.GuildModeration
            }
        }
    }
}
