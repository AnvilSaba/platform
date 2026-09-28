-- Bot に一覧・解除・退出処理で必要な権限だけを追加する。
GRANT SELECT ON mcguildlink.minecraft_accounts, mcguildlink.account_links TO platform_bot_runtime;
GRANT DELETE ON mcguildlink.account_links, mcguildlink.link_requests TO platform_bot_runtime;
GRANT INSERT ON mcguildlink.audit_logs TO platform_bot_runtime;
GRANT USAGE ON SEQUENCE mcguildlink.audit_logs_id_seq TO platform_bot_runtime;

-- 退出イベントの主体は Discord 利用者であり、Minecraft アカウントではない。
ALTER TABLE mcguildlink.audit_logs
    ALTER COLUMN actor_minecraft_uuid DROP NOT NULL,
    ALTER COLUMN actor_minecraft_name DROP NOT NULL;
ALTER TABLE mcguildlink.audit_logs
    ADD COLUMN actor_discord_user_id numeric(20, 0),
    ADD COLUMN actor_discord_username varchar(32);
