-- psql -v ON_ERROR_STOP=1 で管理者として新規の platform DB に適用する。
-- パスワードは別途 \password と Kubernetes Secret で設定する。
SELECT format('CREATE ROLE %I NOLOGIN', name)
FROM unnest(ARRAY['platform_bot_runtime', 'platform_mcguildlink_runtime', 'platform_public_api_runtime']) AS name
WHERE NOT EXISTS (SELECT FROM pg_roles WHERE rolname = name)
\gexec
SELECT format('CREATE ROLE %I LOGIN NOSUPERUSER NOCREATEDB NOCREATEROLE', name)
FROM unnest(ARRAY['platform_migrator_job', 'platform_bot', 'platform_mcguildlink', 'platform_public_api']) AS name
WHERE NOT EXISTS (SELECT FROM pg_roles WHERE rolname = name)
\gexec
REVOKE CREATE ON SCHEMA public FROM PUBLIC;
REVOKE CREATE ON DATABASE platform FROM PUBLIC;
GRANT CONNECT, CREATE ON DATABASE platform TO platform_migrator_job;
GRANT USAGE, CREATE ON SCHEMA public TO platform_migrator_job;
GRANT CONNECT ON DATABASE platform TO platform_bot, platform_mcguildlink, platform_public_api;
GRANT platform_bot_runtime TO platform_bot;
GRANT platform_mcguildlink_runtime TO platform_mcguildlink;
GRANT platform_public_api_runtime TO platform_public_api;
