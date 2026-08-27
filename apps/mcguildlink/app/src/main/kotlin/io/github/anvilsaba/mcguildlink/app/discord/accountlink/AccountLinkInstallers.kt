package io.github.anvilsaba.mcguildlink.app.discord.accountlink

import dev.kord.core.Kord
import io.github.anvilsaba.mcguildlink.app.config.BotConfig
import io.github.anvilsaba.mcguildlink.app.discord.accountlink.commands.installBlockAccountCommand
import io.github.anvilsaba.mcguildlink.app.discord.accountlink.commands.installCreatePanelCommand
import io.github.anvilsaba.mcguildlink.app.discord.accountlink.commands.installListLinksCommand
import io.github.anvilsaba.mcguildlink.app.discord.accountlink.interactions.installAccountLinkButtons
import io.github.anvilsaba.mcguildlink.app.discord.accountlink.interactions.installAccountLinksPagination
import io.github.anvilsaba.mcguildlink.app.discord.accountlink.interactions.installBlockAccountPagination
import io.github.anvilsaba.mcguildlink.app.discord.accountlink.interactions.installUnlinkHandlers
import io.github.anvilsaba.mcguildlink.app.discord.logging.AuditLogSender
import io.github.anvilsaba.mcguildlink.app.discord.registry.InteractionRegistry
import io.github.anvilsaba.mcguildlink.app.service.AccountBlockService
import io.github.anvilsaba.mcguildlink.app.service.AccountLinkService


/**
 * アカウント紐付け機能で使用するインタラクションハンドラとイベントハンドラを登録します。
 */
context(config: BotConfig, accountLinkService: AccountLinkService, accountBlockService: AccountBlockService, auditLogSender: AuditLogSender)
fun installAccountLinkHandlers(
    kord: Kord,
    interactions: InteractionRegistry,
) {
    with(interactions) {
        installAccountLinkButtons()
        installAccountLinksPagination()
        installBlockAccountPagination()
        installUnlinkHandlers()
    }

    with(kord) {
        installAccountLinkMemberLeaveHandler()
    }
}

/**
 * アカウント紐付け機能で使用するギルドコマンドを登録します。
 */
context(config: BotConfig, accountLinkService: AccountLinkService, accountBlockService: AccountBlockService)
suspend fun installCommands(kord: Kord) {
    with(kord) {
        installBlockAccountCommand(config.guild, config.moderatorRole)
        installCreatePanelCommand(config.guild, config.moderatorRole)
        installListLinksCommand(config.guild, config.moderatorRole)
    }
}
