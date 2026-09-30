-- アプリを停止してから管理者で実行する。起動確認中の業務書き込みをDBで拒否する。
BEGIN;
REVOKE platform_bot_runtime FROM platform_bot;
REVOKE platform_mc_link_server_runtime FROM platform_mc_link_server;
GRANT USAGE ON SCHEMA mcguildlink TO platform_bot, platform_mc_link_server;
GRANT SELECT ON mcguildlink.discord_accounts, mcguildlink.minecraft_accounts,
    mcguildlink.account_links, mcguildlink.link_requests,
    mcguildlink.blocked_discord_accounts, mcguildlink.blocked_minecraft_accounts
    TO platform_bot, platform_mc_link_server;
GRANT SELECT ON mcguildlink.block_groups, mcguildlink.audit_logs, mcguildlink.audit_outbox
    TO platform_bot;
GRANT SELECT (version, success, checksum) ON public._sqlx_migrations
    TO platform_bot, platform_mc_link_server;
-- 直接付与・別ロール経由・所有者・superuserなどで書き込み権限が残れば凍結を失敗させる。
DO $$
DECLARE
    app text;
    relation regclass;
BEGIN
    FOREACH app IN ARRAY ARRAY['platform_bot', 'platform_mc_link_server'] LOOP
        FOR relation IN SELECT oid FROM pg_class WHERE relnamespace = 'mcguildlink'::regnamespace
            AND relkind IN ('r', 'p') LOOP
            IF has_table_privilege(app, relation, 'INSERT,UPDATE,DELETE,TRUNCATE')
                OR has_any_column_privilege(app, relation, 'INSERT,UPDATE') THEN
                RAISE EXCEPTION '% に % の書き込み権限が残っています', app, relation;
            END IF;
        END LOOP;
    END LOOP;
END $$;
COMMIT;
