"""専用の空PostgreSQLで移行・照合・書き込み停止・旧SQLiteへの切り戻しを検証する。"""

import argparse
from contextlib import closing
import hashlib
from pathlib import Path
import sqlite3
import subprocess
import sys
import tempfile
import uuid


ROOT = Path(__file__).resolve().parents[1]


def create_fixture(source):
    with closing(sqlite3.connect(source)) as db, db:
        db.executescript("""
            CREATE TABLE discord_accounts (id INTEGER PRIMARY KEY, user_id NUMERIC(20), last_known_username TEXT);
            CREATE TABLE minecraft_accounts (id INTEGER PRIMARY KEY, uuid BLOB, last_known_name TEXT);
            CREATE TABLE account_links (id INTEGER PRIMARY KEY, discord_account_id INTEGER, minecraft_account_id INTEGER, linked_at TEXT);
            CREATE TABLE link_requests (id INTEGER PRIMARY KEY, discord_account_id INTEGER, code TEXT);
            CREATE TABLE block_groups (id INTEGER PRIMARY KEY, root_discord_account_id INTEGER, created_at TEXT);
            CREATE TABLE blocked_discord_accounts (id INTEGER PRIMARY KEY, discord_account_id INTEGER, block_group_id INTEGER);
            CREATE TABLE blocked_minecraft_accounts (id INTEGER PRIMARY KEY, minecraft_account_id INTEGER, block_group_id INTEGER);
            INSERT INTO discord_accounts VALUES (2, 123456789012345678, '利用者'), (7, 223456789012345678, '別アカウント'), (9, 323456789012345678, 'blocked');
            INSERT INTO account_links VALUES (1, 2, 3, '2026-09-30 12:34:56.789'), (2, 7, 3, '2026-09-30 13:00:00.000');
            INSERT INTO link_requests VALUES (1, 2, 'AC234679'), (2, 7, 'BD345689');
            INSERT INTO block_groups VALUES (4, 9, '2026-09-29 12:00:00.123');
            INSERT INTO blocked_discord_accounts VALUES (1, 9, 4);
            INSERT INTO blocked_minecraft_accounts VALUES (1, 8, 4);
        """)
        db.executemany("INSERT INTO minecraft_accounts VALUES (?, ?, ?)", [
            (3, uuid.UUID("12345678-1234-5678-9abc-123456789abc").bytes, "Player"),
            (8, uuid.UUID("aaaaaaaa-bbbb-cccc-dddd-eeeeeeeeeeee").bytes, "Blocked"),
        ])


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--psql", nargs=argparse.REMAINDER,
                        help="専用テストDBに接続するpsqlコマンド（本番では実行しない）")
    parser.add_argument("--fixture-db", type=Path, help="手動リハーサル用の代表SQLiteを作成")
    args = parser.parse_args()
    if args.fixture_db:
        if args.fixture_db.exists():
            parser.error("既存のSQLiteは上書きしません")
        create_fixture(args.fixture_db)
        print("代表SQLiteを作成しました")
        return
    command = args.psql
    assert command, "psqlコマンドが必要です"

    def psql(sql, success=True):
        result = subprocess.run([*command, "-X", "-v", "ON_ERROR_STOP=1", "-At"],
                                input=sql, text=True, encoding="utf-8", capture_output=True)
        assert (result.returncode == 0) == success, result.stderr
        return result.stdout.strip()

    with tempfile.TemporaryDirectory() as temporary:
        source = Path(temporary) / "app.db"
        create_fixture(source)
        # 別経路から付与された権限が残ると凍結は失敗し、半端な権限変更を残さない。
        psql("GRANT UPDATE ON mcguildlink.discord_accounts TO platform_bot")
        psql((ROOT / "scripts/mcguildlink-cutover-freeze.sql").read_text(encoding="utf-8"), success=False)
        psql("REVOKE UPDATE ON mcguildlink.discord_accounts FROM platform_bot")
        psql((ROOT / "scripts/mcguildlink-cutover-freeze.sql").read_text(encoding="utf-8"))

        def export(name, verify=False, success=True):
            output = Path(temporary) / name
            result = subprocess.run([
                sys.executable, str(ROOT / "scripts/migrate-mcguildlink.py"),
                str(source), str(output), "--source-utc-offset=+09:00",
                *(["--verify-only"] if verify else []),
            ], capture_output=True)
            assert (result.returncode == 0) == success, result.stderr.decode(errors="replace")
            return output.read_text(encoding="utf-8") if success else None

        # 最後の紐付けでFK違反が起きても先に取り込んだアカウント・ブロックは残らない。
        with closing(sqlite3.connect(source)) as db, db:
            db.execute("UPDATE account_links SET minecraft_account_id=99 WHERE id=2")
        psql(export("invalid.sql"), success=False)
        assert psql("SELECT count(*) FROM mcguildlink.discord_accounts") == "0"
        assert psql("SELECT count(*) FROM mcguildlink.block_groups") == "0"
        with closing(sqlite3.connect(source)) as db, db:
            db.execute("UPDATE account_links SET minecraft_account_id=3 WHERE id=2")
        before = hashlib.sha256(source.read_bytes()).digest()
        migration = export("import.sql")
        psql(migration)
        psql(export("verify.sql", verify=True))
        assert psql("SELECT count(*) FROM mcguildlink.account_links") == "2"
        assert psql("SELECT uuid FROM mcguildlink.minecraft_accounts WHERE id=3") == "12345678-1234-5678-9abc-123456789abc"
        assert psql("SELECT last_known_username FROM mcguildlink.discord_accounts WHERE id=2") == "利用者"
        assert psql("SET TIME ZONE 'UTC'; SELECT linked_at FROM mcguildlink.account_links WHERE discord_account_id=2") == "SET\n2026-09-30 03:34:56.789+00"
        assert psql("SELECT code FROM mcguildlink.link_requests WHERE discord_account_id=2") == "AC234679"
        assert psql("SELECT root_discord_account_id FROM mcguildlink.block_groups WHERE id=4") == "9"
        assert psql("SELECT discord_account_id FROM mcguildlink.blocked_discord_accounts WHERE block_group_id=4") == "9"
        assert psql("SELECT minecraft_account_id FROM mcguildlink.blocked_minecraft_accounts WHERE block_group_id=4") == "8"
        for table in ("audit_logs", "audit_outbox"):
            assert psql(f"SELECT count(*) FROM mcguildlink.{table}") == "0"
        # ID採番を確認。実際のコード消費はk3dリハーサルで確認する。
        assert "10" in psql("BEGIN; INSERT INTO mcguildlink.discord_accounts (user_id,last_known_username) VALUES (42,'new') RETURNING id; ROLLBACK;").splitlines()
        psql("INSERT INTO mcguildlink.link_requests VALUES (9,'blocked')", success=False)
        psql("INSERT INTO mcguildlink.account_links (discord_account_id,minecraft_account_id) VALUES (2,8)", success=False)
        psql(migration, success=False)  # 再実行で上書きしない。
        psql("UPDATE mcguildlink.discord_accounts SET last_known_username='changed' WHERE id=2")
        psql(export("mismatch.sql", verify=True), success=False)
        psql("UPDATE mcguildlink.discord_accounts SET last_known_username='利用者' WHERE id=2")

        # bootstrap済みの標準LOGINロールで起動確認と同じ権限を検証する。
        psql((ROOT / "scripts/mcguildlink-cutover-freeze.sql").read_text(encoding="utf-8"))
        for role in ("platform_bot", "platform_mc_link_server"):
            assert "2" in psql(f"SET ROLE {role}; SELECT count(*) FROM mcguildlink.link_requests").splitlines()
            for write in ("DELETE FROM mcguildlink.link_requests", "UPDATE mcguildlink.minecraft_accounts SET last_known_name='changed'", "INSERT INTO mcguildlink.discord_accounts (user_id,last_known_username) VALUES (43,'new')"):
                psql(f"SET ROLE {role}; {write}", success=False)
        for table in ("block_groups", "audit_logs", "audit_outbox"):
            psql(f"SET ROLE platform_mc_link_server; SELECT * FROM mcguildlink.{table}", success=False)
        psql(export("frozen-verify.sql", verify=True))
        # 書き込み解禁前は旧DBがそのまま使える。旧版と同じSQLite問い合わせで確認。
        assert hashlib.sha256(source.read_bytes()).digest() == before
        with closing(sqlite3.connect(source)) as old:
            assert old.execute("SELECT code FROM link_requests WHERE discord_account_id=2").fetchone() == ("AC234679",)
            assert old.execute("SELECT count(*) FROM account_links").fetchone() == (2,)
            assert old.execute("SELECT minecraft_account_id FROM blocked_minecraft_accounts").fetchone() == (8,)
        psql((ROOT / "scripts/mcguildlink-cutover-unfreeze.sql").read_text(encoding="utf-8"))
        psql("BEGIN; SET ROLE platform_bot; DELETE FROM mcguildlink.link_requests WHERE code='AC234679'; ROLLBACK;")
        with closing(sqlite3.connect(source)) as db, db:
            db.execute("UPDATE discord_accounts SET user_id=1.2345678901234568e19 WHERE id=2")
        export("lossy.sql", success=False)
        assert not (Path(temporary) / "lossy.sql").exists()
    print("移行・全件照合・ブロック制約・書き込み停止・解禁前の旧DB切り戻し: 成功")


if __name__ == "__main__":
    main()
