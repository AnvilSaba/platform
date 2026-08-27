package io.github.valine3gdev.mcguildlink.app.web

import io.ktor.http.ContentType
import io.ktor.http.HttpStatusCode
import io.ktor.server.application.Application
import io.ktor.server.application.install
import io.ktor.server.http.content.LocalPathContent
import io.ktor.server.plugins.conditionalheaders.ConditionalHeaders
import io.ktor.server.response.respond
import io.ktor.server.routing.get
import io.ktor.server.routing.routing
import java.nio.file.Files
import java.nio.file.Path


/**
 * ホワイトリスト JSON を配信する Ktor ルーティングを構成します。
 */
fun Application.configureWhitelistRouting(whitelistFile: Path) {
    install(ConditionalHeaders)

    routing {
        get("/whitelist.json") {
            if (!Files.exists(whitelistFile) || !Files.isRegularFile(whitelistFile)) {
                call.respond(HttpStatusCode.NotFound)
                return@get
            }

            call.respond(LocalPathContent(whitelistFile, ContentType.Application.Json))
        }
    }
}
