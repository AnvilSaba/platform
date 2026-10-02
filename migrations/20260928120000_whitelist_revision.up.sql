-- 一覧の更新番号を変更と同じトランザクションで確定する。
CREATE TABLE mcguildlink.whitelist_revision (
    singleton boolean PRIMARY KEY DEFAULT true CHECK (singleton),
    revision bigint NOT NULL DEFAULT 0
);
INSERT INTO mcguildlink.whitelist_revision (singleton, revision) VALUES (true, 0);

CREATE FUNCTION mcguildlink.bump_whitelist_revision() RETURNS trigger
LANGUAGE plpgsql SECURITY DEFINER SET search_path = pg_catalog, mcguildlink AS $$
BEGIN
    UPDATE mcguildlink.whitelist_revision SET revision = revision + 1 WHERE singleton;
    RETURN NULL;
END $$;

CREATE TRIGGER whitelist_links_insert AFTER INSERT ON mcguildlink.account_links
    FOR EACH STATEMENT EXECUTE FUNCTION mcguildlink.bump_whitelist_revision();
CREATE TRIGGER whitelist_links_delete AFTER DELETE ON mcguildlink.account_links
    FOR EACH STATEMENT EXECUTE FUNCTION mcguildlink.bump_whitelist_revision();
CREATE TRIGGER whitelist_links_update AFTER UPDATE OF discord_account_id, minecraft_account_id ON mcguildlink.account_links
    FOR EACH STATEMENT EXECUTE FUNCTION mcguildlink.bump_whitelist_revision();
CREATE TRIGGER whitelist_minecraft_update AFTER UPDATE OF uuid, last_known_name ON mcguildlink.minecraft_accounts
    FOR EACH STATEMENT EXECUTE FUNCTION mcguildlink.bump_whitelist_revision();
CREATE TRIGGER whitelist_discord_block_insert AFTER INSERT ON mcguildlink.blocked_discord_accounts
    FOR EACH STATEMENT EXECUTE FUNCTION mcguildlink.bump_whitelist_revision();
CREATE TRIGGER whitelist_discord_block_delete AFTER DELETE ON mcguildlink.blocked_discord_accounts
    FOR EACH STATEMENT EXECUTE FUNCTION mcguildlink.bump_whitelist_revision();
CREATE TRIGGER whitelist_discord_block_update AFTER UPDATE OF discord_account_id ON mcguildlink.blocked_discord_accounts
    FOR EACH STATEMENT EXECUTE FUNCTION mcguildlink.bump_whitelist_revision();
CREATE TRIGGER whitelist_minecraft_block_insert AFTER INSERT ON mcguildlink.blocked_minecraft_accounts
    FOR EACH STATEMENT EXECUTE FUNCTION mcguildlink.bump_whitelist_revision();
CREATE TRIGGER whitelist_minecraft_block_delete AFTER DELETE ON mcguildlink.blocked_minecraft_accounts
    FOR EACH STATEMENT EXECUTE FUNCTION mcguildlink.bump_whitelist_revision();
CREATE TRIGGER whitelist_minecraft_block_update AFTER UPDATE OF minecraft_account_id ON mcguildlink.blocked_minecraft_accounts
    FOR EACH STATEMENT EXECUTE FUNCTION mcguildlink.bump_whitelist_revision();

DO $$ BEGIN
    IF NOT EXISTS (SELECT FROM pg_roles WHERE rolname = 'platform_public_api_runtime') THEN
        CREATE ROLE platform_public_api_runtime NOLOGIN;
    END IF;
EXCEPTION WHEN duplicate_object OR unique_violation THEN
    NULL;
END $$;
GRANT USAGE ON SCHEMA mcguildlink TO platform_public_api_runtime;
GRANT SELECT (version, success, checksum) ON public._sqlx_migrations TO platform_public_api_runtime;
GRANT SELECT ON mcguildlink.whitelist_revision, mcguildlink.account_links,
    mcguildlink.minecraft_accounts, mcguildlink.blocked_discord_accounts,
    mcguildlink.blocked_minecraft_accounts TO platform_public_api_runtime;
