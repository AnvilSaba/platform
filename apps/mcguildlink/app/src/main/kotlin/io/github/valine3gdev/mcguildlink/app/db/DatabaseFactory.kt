package io.github.valine3gdev.mcguildlink.app.db

import org.jetbrains.exposed.v1.jdbc.Database
import org.jetbrains.exposed.v1.jdbc.SchemaUtils
import org.jetbrains.exposed.v1.jdbc.transactions.TransactionManager
import org.jetbrains.exposed.v1.jdbc.transactions.transaction
import java.nio.file.Path
import java.sql.Connection


// TODO: DBのマイグレーションを実装する
/**
 * SQLite データベースへの接続と初期スキーマ作成を担当します。
 */
object DatabaseFactory {
    /**
     * 指定した DB ファイルへ接続し、必要なテーブルとトリガーを作成します。
     */
    fun connect(dbFile: Path): Database {
        return Database.connect(
            url = "jdbc:sqlite:${dbFile.toAbsolutePath()}",
            driver = "org.sqlite.JDBC",
        ).also {
            TransactionManager.manager.defaultIsolationLevel = Connection.TRANSACTION_SERIALIZABLE

            transaction(it) {
                SchemaUtils.create(
                    DiscordAccounts,
                    MinecraftAccounts,
                    AccountLinks,
                    LinkRequests,
                    BlockGroups,
                    BlockedDiscordAccounts,
                    BlockedMinecraftAccounts,
                )

                exec("DROP TRIGGER IF EXISTS prevent_blocked_account_link")
                exec(
                    """
                    CREATE TRIGGER prevent_blocked_account_link
                    BEFORE INSERT ON account_links
                    FOR EACH ROW
                    WHEN EXISTS (
                        SELECT 1
                        FROM blocked_discord_accounts
                        WHERE discord_account_id = NEW.discord_account_id
                    ) OR EXISTS (
                        SELECT 1
                        FROM blocked_minecraft_accounts
                        WHERE minecraft_account_id = NEW.minecraft_account_id
                    )
                    BEGIN
                        SELECT RAISE(ABORT, 'blocked account cannot be linked');
                    END;
                    """.trimIndent()
                )

                exec("DROP TRIGGER IF EXISTS prevent_blocked_link_request")
                exec(
                    """
                    CREATE TRIGGER prevent_blocked_link_request
                    BEFORE INSERT ON link_requests
                    FOR EACH ROW
                    WHEN EXISTS (
                        SELECT 1
                        FROM blocked_discord_accounts
                        WHERE discord_account_id = NEW.discord_account_id
                    )
                    BEGIN
                        SELECT RAISE(ABORT, 'blocked discord account cannot create link request');
                    END;
                    """.trimIndent()
                )
            }
        }
    }
}
