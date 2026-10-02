CREATE TABLE mcguildlink.audit_logs (
    id bigint GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
    event_type text NOT NULL,
    occurred_at timestamptz NOT NULL DEFAULT now(),
    actor_type text NOT NULL,
    actor_minecraft_uuid uuid NOT NULL,
    actor_minecraft_name varchar(16) NOT NULL,
    target_discord_user_id numeric(20, 0) NOT NULL,
    target_discord_username varchar(32) NOT NULL,
    target_minecraft_uuid uuid NOT NULL,
    target_minecraft_name varchar(16) NOT NULL
);

CREATE TABLE mcguildlink.audit_outbox (
    log_id bigint PRIMARY KEY REFERENCES mcguildlink.audit_logs(id),
    retry_count integer NOT NULL DEFAULT 0,
    next_attempt_at timestamptz NOT NULL DEFAULT now(),
    needs_attention boolean NOT NULL DEFAULT false
);

CREATE FUNCTION mcguildlink.enqueue_audit_log() RETURNS trigger
LANGUAGE plpgsql SECURITY DEFINER SET search_path = pg_catalog, mcguildlink AS $$
BEGIN
    INSERT INTO mcguildlink.audit_outbox (log_id) VALUES (NEW.id);
    RETURN NEW;
END $$;

CREATE TRIGGER enqueue_audit_log AFTER INSERT ON mcguildlink.audit_logs
    FOR EACH ROW EXECUTE FUNCTION mcguildlink.enqueue_audit_log();

DO $$ BEGIN
    IF NOT EXISTS (SELECT FROM pg_roles WHERE rolname = 'platform_mc_link_server_runtime') THEN
        CREATE ROLE platform_mc_link_server_runtime NOLOGIN;
    END IF;
EXCEPTION WHEN duplicate_object OR unique_violation THEN
    NULL;
END $$;

GRANT USAGE ON SCHEMA mcguildlink TO platform_mc_link_server_runtime;
GRANT SELECT (version, success, checksum) ON public._sqlx_migrations TO platform_mc_link_server_runtime;
GRANT SELECT ON mcguildlink.discord_accounts, mcguildlink.minecraft_accounts,
    mcguildlink.link_requests, mcguildlink.account_links,
    mcguildlink.blocked_discord_accounts, mcguildlink.blocked_minecraft_accounts
    TO platform_mc_link_server_runtime;
GRANT INSERT ON mcguildlink.minecraft_accounts, mcguildlink.account_links,
    mcguildlink.audit_logs TO platform_mc_link_server_runtime;
GRANT UPDATE (last_known_name) ON mcguildlink.minecraft_accounts TO platform_mc_link_server_runtime;
GRANT DELETE ON mcguildlink.link_requests TO platform_mc_link_server_runtime;
GRANT USAGE ON SEQUENCE mcguildlink.minecraft_accounts_id_seq,
    mcguildlink.audit_logs_id_seq TO platform_mc_link_server_runtime;
