CREATE SCHEMA mcguildlink;
REVOKE ALL ON SCHEMA mcguildlink FROM PUBLIC;

CREATE TABLE mcguildlink.schema_compatibility (
    singleton boolean PRIMARY KEY DEFAULT true CHECK (singleton),
    minimum_version integer NOT NULL,
    maximum_version integer NOT NULL CHECK (maximum_version >= minimum_version)
);
INSERT INTO mcguildlink.schema_compatibility VALUES (true, 1, 1);

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

-- ログイン用ユーザーは運用側が作成し、この権限ロールを付与する。
DO $$ BEGIN
    IF NOT EXISTS (SELECT FROM pg_roles WHERE rolname = 'mcguildlink_bot') THEN
        CREATE ROLE mcguildlink_bot NOLOGIN;
    END IF;
EXCEPTION WHEN duplicate_object OR unique_violation THEN
    -- 独立した DB の初期化が同時に同じクラスタ共有ロールを作る場合。
    NULL;
END $$;
GRANT USAGE ON SCHEMA mcguildlink TO mcguildlink_bot;
GRANT SELECT ON mcguildlink.schema_compatibility, mcguildlink.discord_accounts,
    mcguildlink.link_requests, mcguildlink.blocked_discord_accounts TO mcguildlink_bot;
GRANT INSERT ON mcguildlink.discord_accounts, mcguildlink.link_requests TO mcguildlink_bot;
GRANT UPDATE (last_known_username) ON mcguildlink.discord_accounts TO mcguildlink_bot;
GRANT USAGE ON SEQUENCE mcguildlink.discord_accounts_id_seq TO mcguildlink_bot;
