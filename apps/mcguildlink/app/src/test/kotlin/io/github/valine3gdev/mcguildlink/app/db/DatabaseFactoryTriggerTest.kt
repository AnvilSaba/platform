package io.github.valine3gdev.mcguildlink.app.db

import io.github.valine3gdev.mcguildlink.app.testutil.createTestDatabase
import org.jetbrains.exposed.v1.jdbc.transactions.transaction
import kotlin.test.Test
import kotlin.test.assertEquals
import kotlin.test.assertFails
import kotlin.test.assertTrue
import kotlin.time.Instant
import kotlin.uuid.Uuid


/**
 * DB トリガーがブロック済みアカウントの不正な挿入を拒否することを検証します。
 */
class DatabaseFactoryTriggerTest {
    /**
     * ブロック済み Discord アカウントは紐付けコードを作成できないことを検証します。
     */
    @Test
    fun `link request insert is rejected for blocked discord account`() {
        val db = createTestDatabase()

        val error = assertFails {
            transaction(db) {
                val discord = DiscordAccountEntity.new {
                    userId = 1u
                    lastKnownUsername = "discord-1"
                }
                val blockGroup = BlockGroupEntity.new {
                    rootDiscordAccount = discord
                    createdAt = Instant.parse("2026-01-01T00:00:00Z")
                }
                BlockedDiscordAccountEntity.new {
                    this.blockGroup = blockGroup
                    discordAccount = discord
                }

                LinkRequestEntity.new {
                    discordAccount = discord
                    code = "blocked-request"
                }
            }
        }

        assertTrue(
            error.fullMessage().contains("blocked discord account cannot create link request")
        )
    }

    /**
     * ブロック済み Discord アカウントを含む紐付け挿入が拒否されることを検証します。
     */
    @Test
    fun `account link insert is rejected for blocked discord account`() {
        val db = createTestDatabase()

        val error = assertFails {
            transaction(db) {
                val discord = DiscordAccountEntity.new {
                    userId = 2u
                    lastKnownUsername = "discord-2"
                }
                val minecraft = MinecraftAccountEntity.new {
                    uuid = Uuid.parse("00000000-0000-0000-0000-000000000501")
                    lastKnownName = "Steve"
                }
                val blockGroup = BlockGroupEntity.new {
                    rootDiscordAccount = discord
                    createdAt = Instant.parse("2026-01-01T00:00:00Z")
                }
                BlockedDiscordAccountEntity.new {
                    this.blockGroup = blockGroup
                    discordAccount = discord
                }

                AccountLinkEntity.new {
                    discordAccount = discord
                    minecraftAccount = minecraft
                    linkedAt = Instant.parse("2026-01-01T00:00:00Z")
                }
            }
        }

        assertTrue(error.fullMessage().contains("blocked account cannot be linked"))
    }

    /**
     * ブロック済み Minecraft アカウントを含む紐付け挿入が拒否されることを検証します。
     */
    @Test
    fun `account link insert is rejected for blocked minecraft account`() {
        val db = createTestDatabase()

        val error = assertFails {
            transaction(db) {
                val rootDiscord = DiscordAccountEntity.new {
                    userId = 3u
                    lastKnownUsername = "discord-3"
                }
                val linkingDiscord = DiscordAccountEntity.new {
                    userId = 4u
                    lastKnownUsername = "discord-4"
                }
                val minecraft = MinecraftAccountEntity.new {
                    uuid = Uuid.parse("00000000-0000-0000-0000-000000000502")
                    lastKnownName = "Alex"
                }
                val blockGroup = BlockGroupEntity.new {
                    rootDiscordAccount = rootDiscord
                    createdAt = Instant.parse("2026-01-01T00:00:00Z")
                }
                BlockedMinecraftAccountEntity.new {
                    this.blockGroup = blockGroup
                    minecraftAccount = minecraft
                }

                AccountLinkEntity.new {
                    discordAccount = linkingDiscord
                    minecraftAccount = minecraft
                    linkedAt = Instant.parse("2026-01-01T00:00:00Z")
                }
            }
        }

        assertTrue(error.fullMessage().contains("blocked account cannot be linked"))
    }

    /**
     * ブロックされていないアカウント同士の挿入は成功することを検証します。
     */
    @Test
    fun `non blocked inserts succeed`() {
        val db = createTestDatabase()

        transaction(db) {
            val requestDiscord = DiscordAccountEntity.new {
                userId = 5u
                lastKnownUsername = "discord-5"
            }
            val linkedDiscord = DiscordAccountEntity.new {
                userId = 6u
                lastKnownUsername = "discord-6"
            }
            val minecraft = MinecraftAccountEntity.new {
                uuid = Uuid.parse("00000000-0000-0000-0000-000000000503")
                lastKnownName = "Builder"
            }

            LinkRequestEntity.new {
                discordAccount = requestDiscord
                code = "allowed-request"
            }
            AccountLinkEntity.new {
                discordAccount = linkedDiscord
                minecraftAccount = minecraft
                linkedAt = Instant.parse("2026-01-01T00:00:00Z")
            }
        }

        transaction(db) {
            assertEquals(1, LinkRequestEntity.all().toList().size)
            assertEquals(1, AccountLinkEntity.all().toList().size)
        }
    }
}

/**
 * 例外チェーン全体のメッセージを 1 つの文字列へ連結します。
 */
private fun Throwable.fullMessage(): String =
    generateSequence(this) { it.cause }
        .mapNotNull { it.message }
        .joinToString(" | ")
