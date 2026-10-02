-- BAN の監査記録は Discord アカウントと関連アカウント集合を対象にする。
ALTER TABLE mcguildlink.audit_logs
    ALTER COLUMN target_minecraft_uuid DROP NOT NULL,
    ALTER COLUMN target_minecraft_name DROP NOT NULL;
ALTER TABLE mcguildlink.audit_logs
    ADD COLUMN related_discord_accounts jsonb,
    ADD COLUMN related_minecraft_accounts jsonb;

GRANT SELECT ON mcguildlink.block_groups, mcguildlink.blocked_minecraft_accounts
    TO platform_bot_runtime;
GRANT INSERT, DELETE ON mcguildlink.block_groups,
    mcguildlink.blocked_discord_accounts, mcguildlink.blocked_minecraft_accounts
    TO platform_bot_runtime;
GRANT USAGE ON SEQUENCE mcguildlink.block_groups_id_seq TO platform_bot_runtime;

-- ブロックの関連集合の読み取りと、コード・紐付けの作成を同じ順序で直列化する。
-- trigger にも適用し、アプリを経由しない INSERT で制約を迂回できないようにする。
CREATE FUNCTION mcguildlink.serialize_account_changes() RETURNS void
LANGUAGE plpgsql AS $$
BEGIN
    PERFORM pg_advisory_xact_lock(61061, 1);
END $$;
REVOKE ALL ON FUNCTION mcguildlink.serialize_account_changes() FROM PUBLIC;
GRANT EXECUTE ON FUNCTION mcguildlink.serialize_account_changes()
    TO platform_bot_runtime, platform_mc_link_server_runtime;

CREATE OR REPLACE FUNCTION mcguildlink.reject_blocked_link_request() RETURNS trigger
LANGUAGE plpgsql AS $$
BEGIN
    PERFORM mcguildlink.serialize_account_changes();
    IF EXISTS (SELECT FROM mcguildlink.blocked_discord_accounts
               WHERE discord_account_id = NEW.discord_account_id) THEN
        RAISE EXCEPTION 'blocked discord account cannot create link request' USING ERRCODE = '23514';
    END IF;
    RETURN NEW;
END $$;

CREATE OR REPLACE FUNCTION mcguildlink.reject_blocked_account_link() RETURNS trigger
LANGUAGE plpgsql AS $$
BEGIN
    PERFORM mcguildlink.serialize_account_changes();
    IF EXISTS (SELECT FROM mcguildlink.blocked_discord_accounts
               WHERE discord_account_id = NEW.discord_account_id)
       OR EXISTS (SELECT FROM mcguildlink.blocked_minecraft_accounts
                  WHERE minecraft_account_id = NEW.minecraft_account_id) THEN
        RAISE EXCEPTION 'blocked account cannot be linked' USING ERRCODE = '23514';
    END IF;
    RETURN NEW;
END $$;
