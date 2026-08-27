package io.github.valine3gdev.mcguildlink.app.service

import io.github.valine3gdev.mcguildlink.app.db.AccountLinkEntity
import io.github.valine3gdev.mcguildlink.app.db.BlockGroupEntity
import io.github.valine3gdev.mcguildlink.app.db.BlockedDiscordAccountEntity
import io.github.valine3gdev.mcguildlink.app.db.BlockedMinecraftAccountEntity
import io.github.valine3gdev.mcguildlink.app.db.LinkRequestEntity
import io.github.valine3gdev.mcguildlink.app.service.dto.BlockResult
import io.github.valine3gdev.mcguildlink.app.service.dto.DiscordAccountInfo
import io.github.valine3gdev.mcguildlink.app.service.dto.LinkRequestResult
import io.github.valine3gdev.mcguildlink.app.service.dto.MinecraftAccountInfo
import io.github.valine3gdev.mcguildlink.app.service.dto.UnblockResult
import io.github.valine3gdev.mcguildlink.app.testutil.createLink
import io.github.valine3gdev.mcguildlink.app.testutil.createTestDatabase
import kotlinx.coroutines.runBlocking
import org.jetbrains.exposed.v1.jdbc.transactions.transaction
import kotlin.test.Test
import kotlin.test.assertEquals
import kotlin.test.assertIs
import kotlin.test.assertTrue
import kotlin.time.Instant
import kotlin.uuid.Uuid


/**
 * [AccountBlockService] のブロック連鎖、解除、一覧取得を検証します。
 */
class AccountBlockServiceTest {
    /**
     * 連結成分全体がブロック対象となり、関連リンクとコードも削除されることを検証します。
     */
    @Test
    fun `blockDiscordAccount blocks connected component and removes related links and requests`() = runBlocking {
        val db = createTestDatabase()
        val linkService = AccountLinkService(db)
        val blockService = AccountBlockService(db)
        val sharedMinecraftUuid = Uuid.parse("00000000-0000-0000-0000-000000000401")
        val secondaryMinecraftUuid = Uuid.parse("00000000-0000-0000-0000-000000000402")
        val unaffectedMinecraftUuid = Uuid.parse("00000000-0000-0000-0000-000000000403")

        createLink(linkService, 1u, "discord-1", sharedMinecraftUuid, "Shared")
        createLink(linkService, 2u, "discord-2", sharedMinecraftUuid, "Shared")
        createLink(linkService, 2u, "discord-2", secondaryMinecraftUuid, "Secondary")
        createLink(linkService, 3u, "discord-3", unaffectedMinecraftUuid, "Unaffected")
        assertIs<LinkRequestResult.Success>(linkService.getOrCreateLinkRequest(2u, "discord-2"))
        assertIs<LinkRequestResult.Success>(linkService.getOrCreateLinkRequest(4u, "discord-4"))

        val result = assertIs<BlockResult.Success>(
            blockService.blockDiscordAccount(1u, "discord-1")
        )

        assertEquals(DiscordAccountInfo(1u, "discord-1"), result.blockGroup.rootDiscordAccount)
        assertEquals(
            listOf(
                DiscordAccountInfo(1u, "discord-1"),
                DiscordAccountInfo(2u, "discord-2"),
            ),
            result.blockGroup.blockedDiscordAccountInfos
        )
        assertEquals(
            listOf(
                MinecraftAccountInfo(secondaryMinecraftUuid, "Secondary"),
                MinecraftAccountInfo(sharedMinecraftUuid, "Shared"),
            ),
            result.blockGroup.blockedMinecraftAccountInfos
        )
        assertTrue(linkService.getLinkedMinecraftAccounts(1u).isEmpty())
        assertTrue(linkService.getLinkedMinecraftAccounts(2u).isEmpty())
        assertEquals(
            listOf(MinecraftAccountInfo(unaffectedMinecraftUuid, "Unaffected")),
            linkService.getLinkedMinecraftAccounts(3u)
        )

        val groups = blockService.listBlockedDiscordAccountGroups()
        assertEquals(1, groups.size)
        assertEquals(1u, groups.single().rootDiscordAccount.userId)
        assertEquals(2, groups.single().blockedDiscordAccounts)
        assertEquals(2, groups.single().blockedMinecraftAccounts)

        transaction(db) {
            assertEquals(listOf(4uL), LinkRequestEntity.all().map { it.discordAccount.userId })
            assertEquals(
                listOf(3uL to unaffectedMinecraftUuid),
                AccountLinkEntity.all().map { it.discordAccount.userId to it.minecraftAccount.uuid }
            )
            assertEquals(1, BlockGroupEntity.all().toList().size)
            assertEquals(2, BlockedDiscordAccountEntity.all().toList().size)
            assertEquals(2, BlockedMinecraftAccountEntity.all().toList().size)
        }
    }

    /**
     * すでにブロック済みの連結成分に対する再ブロックが `AlreadyBlocked` を返すことを検証します。
     */
    @Test
    fun `blockDiscordAccount returns already blocked when connected component was already blocked`(): Unit = runBlocking {
        val db = createTestDatabase()
        val linkService = AccountLinkService(db)
        val blockService = AccountBlockService(db)
        val sharedMinecraftUuid = Uuid.parse("00000000-0000-0000-0000-000000000404")

        createLink(linkService, 1u, "discord-1", sharedMinecraftUuid, "Shared")
        createLink(linkService, 2u, "discord-2", sharedMinecraftUuid, "Shared")
        assertIs<BlockResult.Success>(blockService.blockDiscordAccount(1u, "discord-1"))

        assertIs<BlockResult.AlreadyBlocked>(
            blockService.blockDiscordAccount(2u, "discord-2")
        )
    }

    /**
     * ルート以外のメンバーを指定してもブロックグループ全体が解除されることを検証します。
     */
    @Test
    fun `unblockDiscordAccount clears whole block group even for non root member`() = runBlocking {
        val db = createTestDatabase()
        val linkService = AccountLinkService(db)
        val blockService = AccountBlockService(db)
        val sharedMinecraftUuid = Uuid.parse("00000000-0000-0000-0000-000000000405")
        val secondaryMinecraftUuid = Uuid.parse("00000000-0000-0000-0000-000000000406")

        createLink(linkService, 1u, "discord-1", sharedMinecraftUuid, "Shared")
        createLink(linkService, 2u, "discord-2", sharedMinecraftUuid, "Shared")
        createLink(linkService, 2u, "discord-2", secondaryMinecraftUuid, "Secondary")
        assertIs<BlockResult.Success>(blockService.blockDiscordAccount(1u, "discord-1"))

        val result = assertIs<UnblockResult.Success>(
            blockService.unblockDiscordAccount(2u)
        )

        assertEquals(1u, result.blockGroup.rootDiscordAccount.userId)
        assertEquals(
            listOf(
                DiscordAccountInfo(1u, "discord-1"),
                DiscordAccountInfo(2u, "discord-2"),
            ),
            result.blockGroup.blockedDiscordAccountInfos
        )
        assertEquals(
            listOf(
                MinecraftAccountInfo(secondaryMinecraftUuid, "Secondary"),
                MinecraftAccountInfo(sharedMinecraftUuid, "Shared"),
            ),
            result.blockGroup.blockedMinecraftAccountInfos
        )
        assertEquals(2, result.blockGroup.blockedDiscordAccounts)
        assertEquals(2, result.blockGroup.blockedMinecraftAccounts)
        assertTrue(blockService.listBlockedDiscordAccountGroups().isEmpty())
        assertIs<LinkRequestResult.Success>(linkService.getOrCreateLinkRequest(1u, "discord-1"))
        assertIs<UnblockResult.NotBlocked>(blockService.unblockDiscordAccount(1u))

        transaction(db) {
            assertTrue(BlockGroupEntity.all().toList().isEmpty())
            assertTrue(BlockedDiscordAccountEntity.all().toList().isEmpty())
            assertTrue(BlockedMinecraftAccountEntity.all().toList().isEmpty())
        }
    }

    /**
     * ブロックグループ一覧が作成日時の新しい順で返ることを検証します。
     */
    @Test
    fun `listBlockedDiscordAccountGroups returns newest group first`() = runBlocking {
        val db = createTestDatabase()
        val blockService = AccountBlockService(db)

        assertIs<BlockResult.Success>(blockService.blockDiscordAccount(1u, "discord-1"))
        assertIs<BlockResult.Success>(blockService.blockDiscordAccount(3u, "discord-3"))

        transaction(db) {
            val groupsByRoot = BlockGroupEntity.all().associateBy { it.rootDiscordAccount.userId }
            groupsByRoot.getValue(1u).createdAt = Instant.parse("2026-01-01T00:00:00Z")
            groupsByRoot.getValue(3u).createdAt = Instant.parse("2026-01-02T00:00:00Z")
        }

        assertEquals(
            listOf(3uL, 1uL),
            blockService.listBlockedDiscordAccountGroups().map { it.rootDiscordAccount.userId }
        )
    }
}
