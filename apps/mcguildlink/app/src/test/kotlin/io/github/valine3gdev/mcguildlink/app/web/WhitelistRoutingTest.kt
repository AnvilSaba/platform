package io.github.valine3gdev.mcguildlink.app.web

import io.ktor.client.request.get
import io.ktor.client.request.header
import io.ktor.client.statement.bodyAsText
import io.ktor.http.ContentType
import io.ktor.http.HttpHeaders
import io.ktor.http.HttpStatusCode
import io.ktor.server.testing.testApplication
import java.nio.file.Files
import java.nio.file.Path
import java.nio.file.attribute.FileTime
import kotlin.io.path.createTempDirectory
import kotlin.io.path.writeText
import kotlin.test.Test
import kotlin.test.assertEquals
import kotlin.test.assertNotEquals
import kotlin.test.assertNotNull
import kotlin.time.Instant
import kotlin.time.toJavaInstant


/**
 * ホワイトリスト配信ルーティングのレスポンス内容と条件付き GET を検証します。
 */
class WhitelistRoutingTest {
    /**
     * ホワイトリスト取得時に JSON 本文と Last-Modified ヘッダーが返ることを検証します。
     */
    @Test
    fun `GET whitelist returns json with last-modified header`() = testApplication {
        val whitelistFile = createWhitelistFile(
            """
            [{"uuid":"00000000-0000-0000-0000-000000000001","name":"AnotherName"}]
            """.trimIndent(),
            Instant.parse("2026-03-28T00:00:00Z"),
        )

        application {
            configureWhitelistRouting(whitelistFile)
        }

        val response = client.get("/whitelist.json")

        assertEquals(HttpStatusCode.OK, response.status)
        assertEquals(ContentType.Application.Json.toString(), response.headers[HttpHeaders.ContentType])
        assertEquals(
            """[{"uuid":"00000000-0000-0000-0000-000000000001","name":"AnotherName"}]""",
            response.bodyAsText(),
        )
        assertNotNull(response.headers[HttpHeaders.LastModified])
    }

    /**
     * 変更がない場合に `If-Modified-Since` で 304 が返ることを検証します。
     */
    @Test
    fun `If-Modified-Since returns 304 when whitelist is unchanged`() = testApplication {
        val whitelistFile = createWhitelistFile("[]", Instant.parse("2026-03-28T00:00:00Z"))

        application {
            configureWhitelistRouting(whitelistFile)
        }

        val initialResponse = client.get("/whitelist.json")
        val lastModified = assertNotNull(initialResponse.headers[HttpHeaders.LastModified])

        val cachedResponse = client.get("/whitelist.json") {
            header(HttpHeaders.IfModifiedSince, lastModified)
        }

        assertEquals(HttpStatusCode.NotModified, cachedResponse.status)
        assertEquals("", cachedResponse.bodyAsText())
    }

    /**
     * ファイル更新後は新しい本文と更新された Last-Modified が返ることを検証します。
     */
    @Test
    fun `updated whitelist returns new body and last-modified value`() = testApplication {
        val whitelistFile = createWhitelistFile("[]", Instant.parse("2026-03-28T00:00:00Z"))

        application {
            configureWhitelistRouting(whitelistFile)
        }

        val initialResponse = client.get("/whitelist.json")
        val initialLastModified = assertNotNull(initialResponse.headers[HttpHeaders.LastModified])

        whitelistFile.writeText(
            """[{"uuid":"00000000-0000-0000-0000-000000000002","name":"NewName"}]"""
        )
        Files.setLastModifiedTime(whitelistFile, FileTime.from(Instant.parse("2026-03-28T00:00:05Z").toJavaInstant()))

        val updatedResponse = client.get("/whitelist.json") {
            header(HttpHeaders.IfModifiedSince, initialLastModified)
        }
        val updatedLastModified = assertNotNull(updatedResponse.headers[HttpHeaders.LastModified])

        assertEquals(HttpStatusCode.OK, updatedResponse.status)
        assertEquals(
            """[{"uuid":"00000000-0000-0000-0000-000000000002","name":"NewName"}]""",
            updatedResponse.bodyAsText(),
        )
        assertNotEquals(initialLastModified, updatedLastModified)
    }

    /**
     * ホワイトリストファイルが存在しない場合は 404 を返すことを検証します。
     */
    @Test
    fun `missing whitelist returns 404`() = testApplication {
        val root = createTempDirectory("mcguildlink-whitelist-route-")
        val whitelistFile = root.resolve("whitelist.json")

        application {
            configureWhitelistRouting(whitelistFile)
        }

        val response = client.get("/whitelist.json")

        assertEquals(HttpStatusCode.NotFound, response.status)
    }

    /**
     * 指定内容と更新日時を持つ一時ホワイトリストファイルを作成します。
     */
    private fun createWhitelistFile(content: String, lastModifiedAt: Instant): Path {
        val root = createTempDirectory("mcguildlink-whitelist-route-")
        val whitelistFile = root.resolve("whitelist.json")

        whitelistFile.writeText(content)
        Files.setLastModifiedTime(whitelistFile, FileTime.from(lastModifiedAt.toJavaInstant()))

        return whitelistFile
    }
}
