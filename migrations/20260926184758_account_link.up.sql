CREATE SCHEMA mcguildlink;
REVOKE ALL ON SCHEMA mcguildlink FROM PUBLIC;

CREATE TABLE mcguildlink.discord_accounts (
    id bigint GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
    user_id numeric(20, 0) NOT NULL UNIQUE CHECK (user_id BETWEEN 1 AND 18446744073709551615),
    last_known_username varchar(32) NOT NULL
);
CREATE TABLE mcguildlink.minecraft_accounts (
    id bigint GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
    uuid uuid NOT NULL UNIQUE,
    last_known_name varchar(16) NOT NULL
);
CREATE TABLE mcguildlink.account_links (
    discord_account_id bigint NOT NULL REFERENCES mcguildlink.discord_accounts(id),
    minecraft_account_id bigint NOT NULL REFERENCES mcguildlink.minecraft_accounts(id),
    linked_at timestamptz NOT NULL DEFAULT now(),
    PRIMARY KEY (discord_account_id, minecraft_account_id)
);
CREATE INDEX ON mcguildlink.account_links (minecraft_account_id);
CREATE TABLE mcguildlink.link_requests (
    discord_account_id bigint PRIMARY KEY REFERENCES mcguildlink.discord_accounts(id),
    code varchar(64) NOT NULL UNIQUE CHECK (length(code) > 0)
);
CREATE TABLE mcguildlink.block_groups (
    id bigint GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
    root_discord_account_id bigint NOT NULL REFERENCES mcguildlink.discord_accounts(id),
    created_at timestamptz NOT NULL DEFAULT now()
);
CREATE TABLE mcguildlink.blocked_discord_accounts (
    discord_account_id bigint PRIMARY KEY REFERENCES mcguildlink.discord_accounts(id),
    block_group_id bigint NOT NULL REFERENCES mcguildlink.block_groups(id)
);
CREATE TABLE mcguildlink.blocked_minecraft_accounts (
    minecraft_account_id bigint PRIMARY KEY REFERENCES mcguildlink.minecraft_accounts(id),
    block_group_id bigint NOT NULL REFERENCES mcguildlink.block_groups(id)
);

-- アプリの経路を通さない INSERT にも、ブロック済みアカウントの制限を適用する。
CREATE FUNCTION mcguildlink.reject_blocked_link_request() RETURNS trigger
LANGUAGE plpgsql AS $$
BEGIN
    IF EXISTS (SELECT FROM mcguildlink.blocked_discord_accounts
               WHERE discord_account_id = NEW.discord_account_id) THEN
        RAISE EXCEPTION 'blocked discord account cannot create link request' USING ERRCODE = '23514';
    END IF;
    RETURN NEW;
END $$;
CREATE TRIGGER prevent_blocked_link_request BEFORE INSERT ON mcguildlink.link_requests
    FOR EACH ROW EXECUTE FUNCTION mcguildlink.reject_blocked_link_request();

CREATE FUNCTION mcguildlink.reject_blocked_account_link() RETURNS trigger
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
CREATE TRIGGER prevent_blocked_account_link BEFORE INSERT ON mcguildlink.account_links
    FOR EACH ROW EXECUTE FUNCTION mcguildlink.reject_blocked_account_link();

-- ログイン用ユーザーは運用側が作成し、この権限ロールを付与する。
DO $$ BEGIN
    IF NOT EXISTS (SELECT FROM pg_roles WHERE rolname = 'platform_bot_runtime') THEN
        CREATE ROLE platform_bot_runtime NOLOGIN;
    END IF;
EXCEPTION WHEN duplicate_object OR unique_violation THEN
    -- 独立した DB の初期化が同時に同じクラスタ共有ロールを作る場合。
    NULL;
END $$;
GRANT USAGE ON SCHEMA mcguildlink TO platform_bot_runtime;
GRANT SELECT (version, success, checksum) ON public._sqlx_migrations TO platform_bot_runtime;
GRANT SELECT ON mcguildlink.discord_accounts,
    mcguildlink.link_requests, mcguildlink.blocked_discord_accounts TO platform_bot_runtime;
GRANT INSERT ON mcguildlink.discord_accounts, mcguildlink.link_requests TO platform_bot_runtime;
GRANT UPDATE (last_known_username) ON mcguildlink.discord_accounts TO platform_bot_runtime;
GRANT USAGE ON SEQUENCE mcguildlink.discord_accounts_id_seq TO platform_bot_runtime;
