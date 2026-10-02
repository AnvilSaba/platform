"""停止した旧SQLiteから、空DBへの移行または移行後の照合SQLを生成する。Python 3.11+。"""

import argparse
from datetime import datetime, timedelta, timezone
from pathlib import Path
import sqlite3
import uuid


TABLES = {
    "discord_accounts": ("id", "user_id", "last_known_username"),
    "minecraft_accounts": ("id", "uuid", "last_known_name"),
    "block_groups": ("id", "root_discord_account_id", "created_at"),
    "blocked_discord_accounts": ("discord_account_id", "block_group_id"),
    "blocked_minecraft_accounts": ("minecraft_account_id", "block_group_id"),
    "account_links": ("discord_account_id", "minecraft_account_id", "linked_at"),
    "link_requests": ("discord_account_id", "code"),
}
IDENTITIES = ("discord_accounts", "minecraft_accounts", "block_groups")


def utc_offset(value):
    if len(value) != 6 or value[0] not in "+-" or value[3] != ":":
        raise argparse.ArgumentTypeError("UTCオフセットは +00:00 または +09:00 の形式で指定してください")
    try:
        hours, minutes = int(value[1:3]), int(value[4:6])
        if hours > 23 or minutes > 59:
            raise ValueError()
        return timezone(timedelta(minutes=(hours * 60 + minutes) * (1 if value[0] == "+" else -1)))
    except ValueError as error:
        raise argparse.ArgumentTypeError("UTCオフセットが不正です") from error


def literal(column, value, source_timezone):
    cast = ""
    if column == "uuid":
        value = str(uuid.UUID(bytes=value) if isinstance(value, bytes) else uuid.UUID(value))
        cast = "::uuid"
    elif column in ("linked_at", "created_at"):
        instant = datetime.fromisoformat(value)
        if instant.tzinfo is None:
            instant = instant.replace(tzinfo=source_timezone)
        value = instant.astimezone(timezone.utc).isoformat()
        cast = "::timestamptz"
    elif column == "user_id":
        # SQLiteのNUMERICがREALへ丸めたSnowflakeは復元できないので停止する。
        if isinstance(value, float) or not 1 <= int(value) <= 18446744073709551615:
            raise ValueError("Discord IDが不正、またはSQLiteで精度が失われています")
        return str(int(value))
    elif column == "id" or column.endswith("_id"):
        if not isinstance(value, int) or value <= 0:
            raise ValueError("内部IDは正の整数である必要があります")
        return str(value)
    if not isinstance(value, str) or "\0" in value:
        raise ValueError("文字列が不正です")
    # psqlのメタコマンドも含め、入力の文字列をSQLとして解釈させない。
    return f"convert_from(decode('{value.encode('utf-8').hex()}', 'hex'), 'UTF8'){cast}"


def generate(source, source_timezone, verify_only=False):
    tables = (*TABLES, "audit_logs", "audit_outbox")
    sql = ["BEGIN;", "SET LOCAL TIME ZONE 'UTC';",
           "SELECT mcguildlink.serialize_account_changes();",
           "LOCK TABLE " + ", ".join(f"mcguildlink.{t}" for t in tables) + " IN ACCESS EXCLUSIVE MODE;"]
    if not verify_only:
        nonempty = " OR ".join(f"EXISTS (SELECT FROM mcguildlink.{t})" for t in tables)
        sql.append(f"DO $$ BEGIN IF {nonempty} THEN RAISE EXCEPTION '移行先が空ではありません'; END IF; END $$;")
    connection = sqlite3.connect(source.resolve().as_uri() + "?mode=ro", uri=True)
    try:
        connection.execute("BEGIN")
        if connection.execute("PRAGMA integrity_check").fetchall() != [("ok",)]:
            raise ValueError("SQLiteの整合性検査に失敗しました")
        if connection.execute("PRAGMA foreign_key_check").fetchall():
            raise ValueError("SQLiteの参照整合性検査に失敗しました")
        for table, columns in TABLES.items():
            names = ", ".join(columns)
            sql.append(f"CREATE TEMP TABLE expected_{table} ON COMMIT DROP AS SELECT {names} FROM mcguildlink.{table} WITH NO DATA;")
            max_id = 0
            count = 0
            for row in connection.execute(f"SELECT {names} FROM {table}"):
                values = ", ".join(literal(c, v, source_timezone) for c, v in zip(columns, row))
                sql.append(f"INSERT INTO expected_{table} ({names}) VALUES ({values});")
                count += 1
                if table in IDENTITIES:
                    max_id = max(max_id, row[0])
            if not verify_only:
                override = " OVERRIDING SYSTEM VALUE" if table in IDENTITIES else ""
                sql.append(f"INSERT INTO mcguildlink.{table} ({names}){override} SELECT {names} FROM expected_{table};")
                if table in IDENTITIES:
                    sql.append(f"ALTER SEQUENCE mcguildlink.{table}_id_seq RESTART WITH {max_id + 1};")
            sql.append(f"DO $$ BEGIN IF EXISTS ((SELECT {names} FROM mcguildlink.{table} EXCEPT ALL SELECT {names} FROM expected_{table}) UNION ALL (SELECT {names} FROM expected_{table} EXCEPT ALL SELECT {names} FROM mcguildlink.{table})) THEN RAISE EXCEPTION '{table}: 移行内容が一致しません'; END IF; END $$;")
            sql.append(f"SELECT '{table}' AS table_name, {count} AS verified_rows;")
        sql.append("DO $$ BEGIN IF EXISTS (SELECT FROM mcguildlink.audit_logs) OR EXISTS (SELECT FROM mcguildlink.audit_outbox) THEN RAISE EXCEPTION '監査履歴は移行対象外です'; END IF; END $$;")
    finally:
        connection.close()
    sql.append("COMMIT;")
    # ponytail: SQL全体をメモリに保持。大規模DBが対象になったら一時ファイルへ逐次出力する。
    return "\n".join(sql) + "\n"


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("source", type=Path)
    parser.add_argument("output", type=Path)
    parser.add_argument("--source-utc-offset", required=True, type=utc_offset,
                        help="旧JVMのタイムゾーン。標準の旧コンテナは +00:00")
    parser.add_argument("--verify-only", action="store_true")
    args = parser.parse_args()
    sql = generate(args.source, args.source_utc_offset, args.verify_only)
    # 不完全なSQLを出さず、既存のファイルも上書きしない。
    with args.output.open("x", encoding="utf-8", newline="\n") as output:
        output.write(sql)
    print("照合SQLを作成しました" if args.verify_only else "移行SQLを作成しました")


if __name__ == "__main__":
    main()
