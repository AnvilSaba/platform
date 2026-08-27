package io.github.valine3gdev.mcguildlink.app

import java.nio.file.Path
import kotlin.io.path.createDirectories


/**
 * アプリケーションが利用する設定・静的ファイル・データファイルのパス一覧です。
 */
data class AppPaths(
    val appHome: Path,
    val configFile: Path,
    val staticDir: Path,
    val whitelistFile: Path,
    val dataDir: Path,
    val dbFile: Path,
) {
    companion object {
        /**
         * 実行環境に応じてアプリケーションのホームディレクトリを推定し、必要なディレクトリを作成したうえで
         * 利用パスを解決します。
         */
        fun detect(): AppPaths {
            val codeSource = AppPaths::class.java
                .protectionDomain
                .codeSource
                ?.location
                ?.toURI()

            val appHome = if (codeSource != null) {
                val path = Path.of(codeSource)
                val parent = path.parent
                if (parent != null && parent.fileName?.toString() == "lib") {
                    parent.parent
                } else {
                    Path.of(System.getProperty("user.dir"))
                }
            } else {
                Path.of(System.getProperty("user.dir"))
            }

            val configFile = appHome.resolve("config/app.toml")
            val staticDir = appHome.resolve("static")
            val whitelistFile = staticDir.resolve("whitelist.json")
            val dataDir = appHome.resolve("data")
            val dbFile = dataDir.resolve("app.db")

            staticDir.createDirectories()
            dataDir.createDirectories()

            return AppPaths(
                appHome = appHome,
                configFile = configFile,
                staticDir = staticDir,
                whitelistFile = whitelistFile,
                dataDir = dataDir,
                dbFile = dbFile,
            )
        }
    }
}
