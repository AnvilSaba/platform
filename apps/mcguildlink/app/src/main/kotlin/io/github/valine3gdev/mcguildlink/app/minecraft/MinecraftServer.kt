package io.github.valine3gdev.mcguildlink.app.minecraft

import io.github.oshai.kotlinlogging.KotlinLogging
import io.github.valine3gdev.mcguildlink.app.config.MinecraftServerConfig
import io.github.valine3gdev.mcguildlink.app.discord.logging.AuditLogSender
import io.github.valine3gdev.mcguildlink.app.discord.logging.sendLinkSucceeded
import io.github.valine3gdev.mcguildlink.app.service.AccountLinkService
import io.github.valine3gdev.mcguildlink.app.service.dto.LinkResult
import io.github.valine3gdev.mcguildlink.app.service.dto.MinecraftAccountInfo
import net.kyori.adventure.key.Key
import net.kyori.adventure.nbt.CompoundBinaryTag
import net.kyori.adventure.text.Component
import net.kyori.adventure.text.JoinConfiguration
import net.minestom.server.Auth
import net.minestom.server.MinecraftServer
import net.minestom.server.dialog.*
import net.minestom.server.entity.Player
import net.minestom.server.event.player.AsyncPlayerConfigurationEvent
import net.minestom.server.event.player.PlayerConfigCustomClickEvent
import net.minestom.server.event.player.PlayerDisconnectEvent
import net.minestom.server.event.server.ServerListPingEvent
import net.minestom.server.ping.Status
import java.util.*
import java.util.concurrent.CompletableFuture
import java.util.concurrent.ConcurrentHashMap
import kotlin.time.toJavaDuration
import kotlin.uuid.toKotlinUuid


private val logger = KotlinLogging.logger {}


/**
 * Minecraft サーバー上でコード入力 UI を提供し、Discord との紐付けを完了させる起動クラスです。
 */
class MinecraftServer(
    private val config: MinecraftServerConfig,
    private val accountLinkService: AccountLinkService,
    private val auditLogSender: AuditLogSender,
) {
    companion object {
        private val CODE_SUBMIT_KEY = Key.key("mcguildlink:submit_code")
        private val DIALOG_SUCCESS_KEY = Key.key("mcguildlink:dialog_success")
        private const val CODE_INPUT_NBT_KEY = "code"
    }

    /**
     * Minestom サーバーを初期化し、必要なイベントハンドラを登録して起動します。
     */
    fun start() {
        val minecraftServer = MinecraftServer.init(Auth.Online())

        val pendingConfigurationDisconnects = ConcurrentHashMap<UUID, CompletableFuture<Unit>>()

        val globalEventHandler = MinecraftServer.getGlobalEventHandler()
        globalEventHandler.addListener(PlayerConfigCustomClickEvent::class.java) {
            handlePlayerConfigCustomClickEvent(it)
        }
        globalEventHandler.addListener(AsyncPlayerConfigurationEvent::class.java) {
            handleAsyncPlayerConfigurationEvent(pendingConfigurationDisconnects, it)
        }
        globalEventHandler.addListener(PlayerDisconnectEvent::class.java) { event ->
            pendingConfigurationDisconnects.remove(event.player.uuid)?.complete(Unit)
        }
        globalEventHandler.addListener(ServerListPingEvent::class.java) { event ->
            event.status = Status.builder()
                .playerInfo(0, Int.MAX_VALUE)
                .description(Component.text("アカウント紐付け用サーバー"))
                .build()
        }

        minecraftServer.start(config.address, config.port)
    }

    /**
     * プレイヤー接続時にコード入力待ちとタイムアウト監視を設定します。
     */
    private fun handleAsyncPlayerConfigurationEvent(
        pendingConfigurationDisconnects: ConcurrentHashMap<UUID, CompletableFuture<Unit>>,
        event: AsyncPlayerConfigurationEvent
    ) {
        val player = event.player
        val uuid = player.uuid

        val disconnectFuture = CompletableFuture<Unit>()
        pendingConfigurationDisconnects[player.uuid] = disconnectFuture
        val timeoutTask = MinecraftServer.getSchedulerManager()
            .buildTask {
                if (disconnectFuture.isDone || !player.isOnline) {
                    return@buildTask
                }
                logger.warn { "Player ${player.username} ($uuid) timed out during authentication. Disconnecting." }
                player.kick(
                    """
                    コードを入力する時間が長過ぎたため、切断されました。
                    もう一度接続してコードを入力してください。
                    すでにコードを発行している場合、コードの再発行は不要です。
                    """.trimIndent()
                )
            }
            .delay(config.timeout.toJavaDuration())
            .schedule()

        showCodeDialog(player)

        try {
            disconnectFuture.join()
        } finally {
            timeoutTask.cancel()
            pendingConfigurationDisconnects.remove(uuid, disconnectFuture)
        }
    }

    /**
     * ダイアログ送信イベントを受け取り、コード送信や終了操作を処理します。
     */
    private fun handlePlayerConfigCustomClickEvent(event: PlayerConfigCustomClickEvent) {
        val player = event.player
        val username = player.username
        val uuid = player.uuid

        when (event.key) {
            DIALOG_SUCCESS_KEY -> player.kick("正常に切断されました。")

            CODE_SUBMIT_KEY -> {
                val payload = event.payload as? CompoundBinaryTag ?: run {
                    showCodeDialog(
                        player,
                        errorMessage = "コードを受け取れませんでした。もう一度入力してください。"
                    )
                    logger.error { "Player $username ($uuid) submitted invalid payload for code submission." }
                    return
                }

                val code = payload.getString(CODE_INPUT_NBT_KEY).trim()
                if (code.isEmpty()) {
                    showCodeDialog(
                        player,
                        errorMessage = "コードが空です。もう一度入力してください。"
                    )
                    return
                }

                when (val result = accountLinkService.consumeCodeAndLink(code, uuid.toKotlinUuid(), username)) {
                    is LinkResult.InvalidCode -> showCodeDialog(
                        player,
                        code,
                        "無効なコードです。もう一度入力してください。"
                    )

                    is LinkResult.AlreadyLinked -> showAlreadyLinkedDialog(player)

                    is LinkResult.Blocked -> showBlockedDialog(player)

                    is LinkResult.Success -> {
                        logger.info { "Player $username ($uuid) completed authentication successfully, linked with Discord user ${result.discordAccount.lastKnownUsername} (${result.discordAccount.userId})." }
                        auditLogSender.sendLinkSucceeded(
                            result.discordAccount,
                            MinecraftAccountInfo(uuid.toKotlinUuid(), username)
                        )
                        showSuccessDialog(player, result.discordAccount.lastKnownUsername)
                    }
                }
            }

            else -> return
        }
    }

    /**
     * 指定した本文と入力欄から Minestom ダイアログを構築します。
     */
    private fun buildDialog(
        title: String,
        actionLabel: String,
        actionKey: Key,
        bodyMessages: List<String>,
        inputs: List<DialogInput> = emptyList(),
    ): Dialog {
        return Dialog.Notice(
            DialogMetadata(
                Component.text(title),
                null,
                false,
                true,
                DialogAfterAction.CLOSE,
                listOf(
                    DialogBody.PlainMessage(
                        Component.join(
                            JoinConfiguration.newlines(),
                            bodyMessages.map(Component::text)
                        ),
                        256
                    )
                ),
                inputs
            ),
            DialogActionButton(
                Component.text(actionLabel),
                null,
                128,
                DialogAction.DynamicCustom(actionKey, null)
            )
        )
    }

    /**
     * コード入力用ダイアログをプレイヤーへ表示します。
     */
    private fun showCodeDialog(
        player: Player,
        initialCode: String = "",
        errorMessage: String? = null
    ) {
        player.showDialog(
            buildDialog(
                "アカウント認証", "送信", CODE_SUBMIT_KEY,
                buildList {
                    add("発行されたコードを以下に入力してください。")
                    errorMessage?.let { add(it) }
                },
                listOf(
                    DialogInput.Text(
                        CODE_INPUT_NBT_KEY,
                        128,
                        Component.text("コード"),
                        true,
                        initialCode,
                        8,
                        null
                    )
                )
            )
        )
    }

    /**
     * 紐付け成功メッセージを表示します。
     */
    private fun showSuccessDialog(player: Player, discordUsername: String) {
        player.showDialog(
            buildDialog(
                "成功しました。",
                "切断",
                DIALOG_SUCCESS_KEY,
                listOf("$discordUsername との紐付けが完了しました。")
            )
        )
    }

    /**
     * すでに同じ組み合わせが紐付け済みであることを通知します。
     */
    private fun showAlreadyLinkedDialog(player: Player) {
        player.showDialog(
            buildDialog(
                "既に紐付け済みです。",
                "切断",
                DIALOG_SUCCESS_KEY,
                listOf("このMinecraftアカウントとこのDiscordアカウントは既に紐付けられています。")
            )
        )
    }

    /**
     * ブロック済みアカウントのため紐付けできないことを通知します。
     */
    private fun showBlockedDialog(player: Player) {
        player.showDialog(
            buildDialog(
                "紐付けできません。",
                "切断",
                DIALOG_SUCCESS_KEY,
                listOf("このアカウントはブロックされているため、紐付けを続行できません。")
            )
        )
    }
}
