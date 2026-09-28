REVOKE USAGE ON SEQUENCE mcguildlink.block_groups_id_seq FROM platform_bot_runtime;
REVOKE INSERT, DELETE ON mcguildlink.block_groups,
    mcguildlink.blocked_discord_accounts, mcguildlink.blocked_minecraft_accounts
    FROM platform_bot_runtime;
REVOKE SELECT ON mcguildlink.block_groups, mcguildlink.blocked_minecraft_accounts
    FROM platform_bot_runtime;

CREATE OR REPLACE FUNCTION mcguildlink.reject_blocked_link_request() RETURNS trigger
LANGUAGE plpgsql AS $$
BEGIN
    IF EXISTS (SELECT FROM mcguildlink.blocked_discord_accounts
               WHERE discord_account_id = NEW.discord_account_id) THEN
        RAISE EXCEPTION 'blocked discord account cannot create link request' USING ERRCODE = '23514';
    END IF;
    RETURN NEW;
END $$;
CREATE OR REPLACE FUNCTION mcguildlink.reject_blocked_account_link() RETURNS trigger
LANGUAGE plpgsql AS $$
BEGIN
    IF EXISTS (SELECT FROM mcguildlink.blocked_discord_accounts
               WHERE discord_account_id = NEW.discord_account_id)
       OR EXISTS (SELECT FROM mcguildlink.blocked_minecraft_accounts
                  WHERE minecraft_account_id = NEW.minecraft_account_id) THEN
        RAISE EXCEPTION 'blocked account cannot be linked' USING ERRCODE = '23514';
    END IF;
    RETURN NEW;
END $$;
DROP FUNCTION mcguildlink.serialize_account_changes();

DELETE FROM mcguildlink.audit_outbox
WHERE log_id IN (SELECT id FROM mcguildlink.audit_logs WHERE event_type = 'member_banned_blocked');
DELETE FROM mcguildlink.audit_logs WHERE event_type = 'member_banned_blocked';
ALTER TABLE mcguildlink.audit_logs
    DROP COLUMN related_discord_accounts,
    DROP COLUMN related_minecraft_accounts;
ALTER TABLE mcguildlink.audit_logs
    ALTER COLUMN target_minecraft_uuid SET NOT NULL,
    ALTER COLUMN target_minecraft_name SET NOT NULL;
