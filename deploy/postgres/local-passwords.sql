-- ローカル開発専用。bootstrap.sql の後に実行する。
ALTER ROLE platform_db_migrator PASSWORD 'migrator-dev-password';
ALTER ROLE platform_bot PASSWORD 'bot-dev-password';
ALTER ROLE platform_mc_link_server PASSWORD 'mc-dev-password';
ALTER ROLE platform_public_api PASSWORD 'api-dev-password';
