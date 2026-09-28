-- 一覧の更新番号を変更と同じトランザクションで確定する。
CREATE TABLE mcguildlink.whitelist_revision (
    singleton boolean PRIMARY KEY DEFAULT true CHECK (singleton),
    revision bigint NOT NULL DEFAULT 0,
    last_modified_at timestamptz NOT NULL DEFAULT clock_timestamp(),
    ims_safe boolean NOT NULL DEFAULT true
);
INSERT INTO mcguildlink.whitelist_revision (singleton, revision) VALUES (true, 0);

CREATE FUNCTION mcguildlink.bump_whitelist_revision() RETURNS trigger
LANGUAGE plpgsql SECURITY DEFINER SET search_path = pg_catalog, mcguildlink AS $$
DECLARE
    previous_at timestamptz;
    next_at timestamptz;
BEGIN
    -- 更新番号の行ロックにより、同時トランザクションでも日時の順序を保つ。
    SELECT last_modified_at INTO previous_at
      FROM mcguildlink.whitelist_revision WHERE singleton FOR UPDATE;
    next_at := GREATEST(clock_timestamp(), previous_at);
    UPDATE mcguildlink.whitelist_revision
       SET revision = revision + 1,
           last_modified_at = next_at,
           -- HTTP-date は秒精度。同じ秒の変更には If-Modified-Since を使わない。
           ims_safe = date_trunc('second', next_at) > date_trunc('second', previous_at)
     WHERE singleton;
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
