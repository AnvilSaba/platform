package io.github.valine3gdev.mcguildlink.app.discord.logging

import dev.kord.common.Color
import dev.kord.common.DiscordTimestampStyle
import dev.kord.common.entity.MessageFlag
import dev.kord.common.entity.Snowflake
import dev.kord.common.toMessageFormat
import dev.kord.core.Kord
import dev.kord.core.entity.User
import dev.kord.rest.builder.component.ContainerBuilder
import dev.kord.rest.builder.component.section
import dev.kord.rest.builder.component.separator
import dev.kord.rest.builder.message.container
import dev.kord.rest.builder.message.messageFlags
import io.github.oshai.kotlinlogging.KotlinLogging
import io.github.valine3gdev.mcguildlink.app.discord.util.formatDiscordAccounts
import io.github.valine3gdev.mcguildlink.app.discord.util.formatMinecraftAccounts
import io.github.valine3gdev.mcguildlink.app.service.dto.BlockResult
import io.github.valine3gdev.mcguildlink.app.service.dto.DiscordAccountInfo
import io.github.valine3gdev.mcguildlink.app.service.dto.MinecraftAccountInfo
import io.github.valine3gdev.mcguildlink.app.service.dto.toHeadAvatarUrl
import kotlinx.coroutines.CancellationException
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.launch
import kotlin.time.Clock
import kotlin.time.Instant


private val logger = KotlinLogging.logger {}

/**
 * 監査ログ向けメッセージ送信の抽象化です。
 */
interface AuditLogSender {
    /**
     * 指定内容の監査ログを送信します。
     */
    fun send(timestamp: Instant = Clock.System.now(), entry: ContainerBuilder.() -> Unit)

    /**
     * 情報レベルの監査ログを送信します。
     */
    fun sendInfo(timestamp: Instant = Clock.System.now(), entry: ContainerBuilder.() -> Unit) = send(timestamp) {
        entry.invoke(this)
        this.accentColor = Color(87, 242, 135)
    }

    /**
     * 警告レベルの監査ログを送信します。
     */
    fun sendWarn(timestamp: Instant = Clock.System.now(), entry: ContainerBuilder.() -> Unit) = send(timestamp) {
        entry.invoke(this)
        this.accentColor = Color(254, 231, 92)
    }

    /**
     * エラーレベルの監査ログを送信します。
     */
    fun sendError(timestamp: Instant = Clock.System.now(), entry: ContainerBuilder.() -> Unit) = send(timestamp) {
        entry.invoke(this)
        this.accentColor = Color(237, 66, 69)
    }
}

/**
 * Discord チャンネルへ監査ログメッセージを非同期送信する実装です。
 */
class DiscordAuditLogSender(
    private val kord: Kord,
    private val channelId: Snowflake,
    private val scope: CoroutineScope,
) : AuditLogSender {
    /**
     * 監査ログ用チャンネルへ Components V2 メッセージを送信します。
     */
    override fun send(timestamp: Instant, entry: ContainerBuilder.() -> Unit) {
        scope.launch {
            try {
                kord.rest.channel.createMessage(channelId) {
                    messageFlags {
                        +MessageFlag.IsComponentsV2
                    }

                    container {
                        entry.invoke(this)
                        separator {
                            divider = false
                        }
                        textDisplay("-# ${timestamp.toMessageFormat(DiscordTimestampStyle.LongDateTime)}")
                    }
                }
            } catch (e: CancellationException) {
                throw e
            } catch (e: Exception) {
                logger.warn(e) { "Failed to send audit log to channel $channelId." }
            }
        }
    }
}

/**
 * 紐付け成功ログを送信します。
 */
fun AuditLogSender.sendLinkSucceeded(discordAccount: DiscordAccountInfo, minecraftAccount: MinecraftAccountInfo) =
    sendInfo {
        textDisplay("下記のアカウントの紐付けが完了しました。")

        separator {
            divider = false
        }

        section {
            textDisplay(
                """
                - Discordユーザー
                  - ${discordAccount.lastKnownUsername} (`${discordAccount.userId}`)
                - Minecraftアカウント
                  - ${minecraftAccount.lastKnownName} (`${minecraftAccount.uuid}`)
                """.trimIndent()
            )

            thumbnailAccessory {
                url = minecraftAccount.toHeadAvatarUrl()
            }
        }
    }

/**
 * メンバー退出に伴う自動解除ログを送信します。
 */
fun AuditLogSender.sendMemberLeaveUnlinked(leftUser: User, minecraftAccounts: List<MinecraftAccountInfo>) =
    sendInfo {
        textDisplay("メンバーの退出を検知したため、下記アカウントの紐付けを自動解除しました。")

        separator {
            divider = false
        }

        textDisplay(
            """
            |- Discordユーザー
            |  - ${leftUser.username} (`${leftUser.id}`)
            |- Minecraftアカウント
            |${minecraftAccounts.formatMinecraftAccounts("  - ")}
            """.trimMargin()
        )
    }

/**
 * メンバー BAN に伴う自動ブロックログを送信します。
 */
fun AuditLogSender.sendMemberBannedBlocked(result: BlockResult.Success) =
    sendInfo {
        val (discordAccount, blockedDiscordAccounts, blockedMinecraftAccounts) = result.blockGroup

        textDisplay("メンバーのBANを検知したため、関連アカウントを自動ブロックしました。")

        separator {
            divider = false
        }

        textDisplay(
            buildString {
                appendLine(
                    """
                    - BAN対象の Discordユーザー
                      - $discordAccount)
                    """.trimIndent()
                )

                if (blockedDiscordAccounts.isNotEmpty()) {
                    appendLine()
                    appendLine(
                        """
                        - ブロックした Discordアカウント
                        ${blockedDiscordAccounts.formatDiscordAccounts("  - ")}
                        """.trimIndent()
                    )
                }

                if (blockedMinecraftAccounts.isNotEmpty()) {
                    appendLine()
                    appendLine(
                        """
                        - ブロックした Minecraftアカウント
                        ${blockedMinecraftAccounts.formatMinecraftAccounts("  - ")}
                        """.trimIndent()
                    )
                }
            }.trimEnd()
        )
    }
