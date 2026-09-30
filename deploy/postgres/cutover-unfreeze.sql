-- 照合・起動確認を完了し、旧版へ戻さないと決めた後、アプリを停止して管理者で実行する。
BEGIN;
REVOKE SELECT ON mcguildlink.discord_accounts, mcguildlink.minecraft_accounts,
    mcguildlink.account_links, mcguildlink.link_requests,
    mcguildlink.blocked_discord_accounts, mcguildlink.blocked_minecraft_accounts
    FROM platform_bot, platform_mc_link_server;
REVOKE SELECT ON mcguildlink.block_groups, mcguildlink.audit_logs, mcguildlink.audit_outbox
    FROM platform_bot;
REVOKE USAGE ON SCHEMA mcguildlink FROM platform_bot, platform_mc_link_server;
REVOKE SELECT (version, success, checksum) ON public._sqlx_migrations
    FROM platform_bot, platform_mc_link_server;
GRANT platform_bot_runtime TO platform_bot;
GRANT platform_mc_link_server_runtime TO platform_mc_link_server;
COMMIT;
