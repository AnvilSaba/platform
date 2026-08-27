package io.github.valine3gdev.mcguildlink.app.service

import io.github.oshai.kotlinlogging.KotlinLogging
import io.github.valine3gdev.mcguildlink.app.db.AccountLinkEntity
import kotlinx.coroutines.CancellationException
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.Job
import kotlinx.coroutines.NonCancellable
import kotlinx.coroutines.currentCoroutineContext
import kotlinx.coroutines.ensureActive
import kotlinx.coroutines.flow.MutableSharedFlow
import kotlinx.coroutines.flow.collectLatest
import kotlinx.coroutines.launch
import kotlinx.coroutines.withContext
import kotlinx.serialization.Serializable
import kotlinx.serialization.json.Json
import org.jetbrains.exposed.v1.jdbc.Database
import org.jetbrains.exposed.v1.jdbc.transactions.suspendTransaction
import java.nio.file.Files
import java.nio.file.Path
import java.nio.file.StandardCopyOption.ATOMIC_MOVE
import java.nio.file.StandardCopyOption.REPLACE_EXISTING
import kotlin.io.path.createDirectories
import kotlin.io.path.deleteIfExists
import kotlin.io.path.writeText


private val logger = KotlinLogging.logger {}

/**
 * DB 上の紐付け状態から Minecraft ホワイトリストファイルを再生成する内部サービスです。
 */
internal class WhitelistFileSyncService(
    private val db: Database,
    private val whitelistFile: Path,
    private val beforeMove: suspend () -> Unit = {},
    private val moveFile: (Path, Path) -> Unit = { source, target ->
        Files.move(source, target, ATOMIC_MOVE, REPLACE_EXISTING)
    },
) : WhitelistRefreshRequester {
    private val json = Json {
        prettyPrint = true
    }

    private val refreshRequests = MutableSharedFlow<Unit>(
        replay = 0,
        extraBufferCapacity = 1,
        onBufferOverflow = kotlinx.coroutines.channels.BufferOverflow.DROP_OLDEST,
    )

    /**
     * 最新状態でホワイトリストを再生成するよう通知します。
     */
    override fun requestRefresh() {
        refreshRequests.tryEmit(Unit)
    }

    /**
     * 現在の紐付け状態を読み取り、ホワイトリスト JSON を即時生成します。
     */
    suspend fun generateNow() {
        val entries = suspendTransaction(db) {
            AccountLinkEntity.all()
                .mapNotNull { link ->
                    val discord = link.discordAccount
                    val minecraft = link.minecraftAccount
                    if (discord.blockedMembership != null || minecraft.blockedMembership != null) {
                        null
                    } else {
                        minecraft
                    }
                }
                .distinctBy { it.uuid }
                .map { minecraft ->
                    MinecraftWhitelistEntry(
                        uuid = minecraft.uuid.toHexDashString(),
                        name = minecraft.lastKnownName,
                    )
                }
                .sortedBy { it.uuid }
        }

        writeWhitelist(entries)
        logger.info { "Generated whitelist file at $whitelistFile with ${entries.size} entries." }
    }

    /**
     * 再生成要求を購読し、常に最新の要求だけを反映する同期ジョブを開始します。
     */
    fun attach(scope: CoroutineScope): Job = scope.launch {
        refreshRequests.collectLatest {
            try {
                generateNow()
            } catch (e: CancellationException) {
                throw e
            } catch (e: Exception) {
                logger.error(e) { "Failed to refresh whitelist file at $whitelistFile." }
            }
        }
    }

    /**
     * ホワイトリスト JSON を一時ファイルへ書き出し、アトミックに差し替えます。
     */
    private suspend fun writeWhitelist(entries: List<MinecraftWhitelistEntry>) {
        withContext(Dispatchers.IO) {
            val parent = whitelistFile.parent ?: error("Whitelist file must have a parent directory: $whitelistFile")
            val temporaryFile = whitelistFile.resolveSibling("${whitelistFile.fileName}.tmp")

            parent.createDirectories()

            try {
                temporaryFile.writeText(json.encodeToString(entries))
                currentCoroutineContext().ensureActive()
                beforeMove()
                currentCoroutineContext().ensureActive()
                moveFile(temporaryFile, whitelistFile)
            } finally {
                withContext(NonCancellable) {
                    temporaryFile.deleteIfExists()
                }
            }
        }
    }
}


@Serializable
/**
 * `whitelist.json` へ出力する Minecraft アカウント情報です。
 */
internal data class MinecraftWhitelistEntry(
    val uuid: String,
    val name: String,
)
