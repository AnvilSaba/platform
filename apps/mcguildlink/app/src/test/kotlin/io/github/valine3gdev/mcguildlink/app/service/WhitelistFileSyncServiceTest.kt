package io.github.valine3gdev.mcguildlink.app.service

import io.github.valine3gdev.mcguildlink.app.service.dto.BlockResult
import io.github.valine3gdev.mcguildlink.app.testutil.TestEnvironment
import io.github.valine3gdev.mcguildlink.app.testutil.createLink
import kotlinx.coroutines.CompletableDeferred
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.SupervisorJob
import kotlinx.coroutines.cancel
import kotlinx.coroutines.delay
import kotlinx.coroutines.runBlocking
import kotlinx.coroutines.withTimeout
import kotlinx.serialization.json.Json
import kotlinx.serialization.json.jsonArray
import kotlinx.serialization.json.jsonObject
import kotlinx.serialization.json.jsonPrimitive
import java.io.IOException
import kotlin.io.path.createDirectories
import kotlin.io.path.exists
import kotlin.io.path.readText
import kotlin.io.path.writeText
import kotlin.test.Test
import kotlin.test.assertEquals
import kotlin.test.assertFalse
import kotlin.test.assertIs
import kotlin.test.assertTrue
import kotlin.test.assertFailsWith
import kotlin.time.Duration.Companion.milliseconds
import kotlin.uuid.Uuid


/**
 * [WhitelistFileSyncService] の生成内容と再生成制御を検証します。
 */
class WhitelistFileSyncServiceTest {
    /**
     * リンクが 1 件もない場合に空配列のホワイトリストが出力されることを検証します。
     */
    @Test
    fun `generateNow writes empty whitelist when no links exist`() = runBlocking {
        val environment = TestEnvironment()
        val service = WhitelistFileSyncService(environment.db, environment.whitelistFile)

        service.generateNow()

        assertEquals("[]", environment.whitelistFile.readText().trim())
    }

    /**
     * UUID 重複を除去しつつ UUID 昇順で出力し、Minecraft 名の大文字小文字を保持することを検証します。
     */
    @Test
    fun `generateNow writes sorted deduplicated entries and preserves name casing`() = runBlocking {
        val environment = TestEnvironment()
        val linkService = AccountLinkService(environment.db)
        val service = WhitelistFileSyncService(environment.db, environment.whitelistFile)

        createLink(
            linkService = linkService,
            discordUserId = 1u,
            discordUsername = "discord-1",
            minecraftUuid = Uuid.parse("00000000-0000-0000-0000-000000000002"),
            minecraftName = "MiXeDCaSe",
        )
        createLink(
            linkService = linkService,
            discordUserId = 2u,
            discordUsername = "discord-2",
            minecraftUuid = Uuid.parse("00000000-0000-0000-0000-000000000002"),
            minecraftName = "MiXeDCaSe",
        )
        createLink(
            linkService = linkService,
            discordUserId = 3u,
            discordUsername = "discord-3",
            minecraftUuid = Uuid.parse("00000000-0000-0000-0000-000000000001"),
            minecraftName = "AnotherName",
        )

        service.generateNow()

        val entries = Json.parseToJsonElement(environment.whitelistFile.readText())
            .jsonArray
            .map { element ->
                element.jsonObject.mapValues { (_, value) -> value.jsonPrimitive.content }
            }

        assertEquals(2, entries.size)
        assertEquals("00000000-0000-0000-0000-000000000001", entries[0].getValue("uuid"))
        assertEquals("AnotherName", entries[0].getValue("name"))
        assertEquals("00000000-0000-0000-0000-000000000002", entries[1].getValue("uuid"))
        assertEquals("MiXeDCaSe", entries[1].getValue("name"))
    }

    /**
     * ブロック済みアカウントはホワイトリスト出力から除外されることを検証します。
     */
    @Test
    fun `generateNow removes blocked accounts from whitelist output`() = runBlocking {
        val environment = TestEnvironment()
        val linkService = AccountLinkService(environment.db)
        val blockService = AccountBlockService(environment.db)
        val service = WhitelistFileSyncService(environment.db, environment.whitelistFile)

        createLink(
            linkService = linkService,
            discordUserId = 1u,
            discordUsername = "discord-1",
            minecraftUuid = Uuid.parse("00000000-0000-0000-0000-000000000010"),
            minecraftName = "BlockedName",
        )
        assertIs<BlockResult.Success>(blockService.blockDiscordAccount(1u, "discord-1"))

        service.generateNow()

        assertEquals("[]", environment.whitelistFile.readText().trim())
    }

    /**
     * アトミック移動失敗時に既存ファイルを保持し、一時ファイルが掃除されることを検証します。
     */
    @Test
    fun `generateNow keeps previous whitelist when atomic move fails and cleans up temp file`() = runBlocking {
        val environment = TestEnvironment()
        val linkService = AccountLinkService(environment.db)
        val service = WhitelistFileSyncService(
            db = environment.db,
            whitelistFile = environment.whitelistFile,
            moveFile = { _, _ -> throw IOException("boom") },
        )

        environment.whitelistFile.parent.createDirectories()
        environment.whitelistFile.writeText(
            """
            [
              {
                "uuid": "00000000-0000-0000-0000-000000000099",
                "name": "Existing"
              }
            ]
            """.trimIndent()
        )

        createLink(
            linkService = linkService,
            discordUserId = 1u,
            discordUsername = "discord-1",
            minecraftUuid = Uuid.parse("00000000-0000-0000-0000-000000000001"),
            minecraftName = "NewName",
        )

        assertFailsWith<IOException> {
            service.generateNow()
        }

        assertTrue(environment.whitelistFile.readText().contains("Existing"))
        assertFalse(environment.whitelistFile.resolveSibling("whitelist.json.tmp").exists())
    }

    /**
     * 連続した再生成要求では古い処理が打ち切られ、最終状態だけがファイルへ反映されることを検証します。
     */
    @Test
    fun `attach cancels stale refresh and writes latest state`() = runBlocking {
        val environment = TestEnvironment()
        val setupLinkService = AccountLinkService(environment.db)
        val firstRefreshReachedMove = CompletableDeferred<Unit>()
        var beforeMoveCalls = 0
        val service = WhitelistFileSyncService(
            db = environment.db,
            whitelistFile = environment.whitelistFile,
            beforeMove = {
                beforeMoveCalls += 1
                if (beforeMoveCalls == 1) {
                    firstRefreshReachedMove.complete(Unit)
                    CompletableDeferred<Unit>().await()
                }
            },
        )
        val notifyingLinkService = AccountLinkService(
            db = environment.db,
            whitelistRefreshRequester = service,
        )
        val scope = CoroutineScope(SupervisorJob() + Dispatchers.Default)
        val collectorJob = service.attach(scope)
        val minecraftUuid = Uuid.parse("00000000-0000-0000-0000-000000000050")

        try {
            createLink(
                linkService = setupLinkService,
                discordUserId = 1u,
                discordUsername = "discord-1",
                minecraftUuid = minecraftUuid,
                minecraftName = "LatestOnly",
            )

            service.requestRefresh()
            firstRefreshReachedMove.await()
            notifyingLinkService.unlink(1u, minecraftUuid)

            withTimeout(5_000.milliseconds) {
                while (!environment.whitelistFile.exists() || environment.whitelistFile.readText().trim() != "[]") {
                    delay(50.milliseconds)
                }
            }

            assertTrue(beforeMoveCalls >= 2)
            assertFalse(environment.whitelistFile.resolveSibling("whitelist.json.tmp").exists())
        } finally {
            scope.cancel()
            collectorJob.join()
        }
    }
}
