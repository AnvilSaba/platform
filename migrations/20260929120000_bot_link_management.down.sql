ALTER TABLE mcguildlink.audit_logs
    DROP COLUMN actor_discord_username,
    DROP COLUMN actor_discord_user_id;
UPDATE mcguildlink.audit_logs
    SET actor_minecraft_uuid = '00000000-0000-0000-0000-000000000000',
        actor_minecraft_name = 'unknown'
    WHERE actor_minecraft_uuid IS NULL;
ALTER TABLE mcguildlink.audit_logs
    ALTER COLUMN actor_minecraft_uuid SET NOT NULL,
    ALTER COLUMN actor_minecraft_name SET NOT NULL;
REVOKE USAGE ON SEQUENCE mcguildlink.audit_logs_id_seq FROM platform_bot_runtime;
REVOKE INSERT ON mcguildlink.audit_logs FROM platform_bot_runtime;
REVOKE DELETE ON mcguildlink.account_links, mcguildlink.link_requests FROM platform_bot_runtime;
REVOKE SELECT ON mcguildlink.minecraft_accounts, mcguildlink.account_links FROM platform_bot_runtime;
