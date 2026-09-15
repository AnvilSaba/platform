"""設計草案のエディタ向け JSON Schema を生成する。Bot の実装ではない。"""
import json
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
ID = r"^[A-Za-z0-9_-]+$"


def obj(properties, required=()):
    result = {"type": "object", "properties": properties, "additionalProperties": False}
    if required:
        result["required"] = list(required)
    return result


def mapping(value, pattern=ID):
    return {"type": "object", "patternProperties": {pattern: value}, "additionalProperties": False}


def ref(name):
    return {"$ref": f"#/definitions/{name}"}


def managed(value, clear=False):
    choices = [value, ref("default")]
    if clear:
        choices.append(ref("clear"))
    return {"oneOf": choices}


string = {"type": "string"}
boolean = {"type": "boolean"}
integer = {"type": "integer", "minimum": 0}
logical_id = {"type": "string", "pattern": ID}
id_list = {"type": "array", "items": logical_id, "uniqueItems": True}


def resource(properties):
    active = obj({"mode": {"const": "managed"}, "ensure": {"const": "present"}, **properties})
    return {"oneOf": [active, obj({"mode": {"const": "reference"}}, ["mode"]),
                       obj({"ensure": {"const": "absent"}}, ["ensure"])]}


definitions = {
    "default": obj({"default": {"const": True}}, ["default"]),
    "clear": obj({"clear": {"const": True}}, ["clear"]),
    "message": obj({"id": logical_id, "body": {"type": "string", "minLength": 1}}, ["id", "body"]),
    "messages": {"type": "array", "items": ref("message"), "description": "各要素が1投稿。ID重複は意味検証で拒否。"},
    "thread": {"oneOf": [
        obj({"channel": logical_id, "name": {"type": "string", "minLength": 1, "maxLength": 100}, "body": ref("messages")}, ["channel", "name", "body"]),
        obj({"ensure": {"const": "absent"}}, ["ensure"]),
    ]},
    "overwrite": mapping({"enum": ["allow", "deny", "clear"]}, r"^[A-Z][A-Z0-9_]*$"),
    "tag": resource({"name": managed(string), "moderated": managed(boolean), "emoji": managed(string, True)}),
}
role_attributes = {
    "name": managed(string), "color": managed({"type": "integer", "minimum": 0, "maximum": 16777215}),
    "hoist": managed(boolean), "mentionable": managed(boolean),
    "permissions": mapping(managed(boolean), r"^[A-Z][A-Z0-9_]*$"),
}
definitions["role_settings_set"] = obj(role_attributes)
definitions["role"] = resource({
    **role_attributes,
    "settings_sets": {**id_list, "description": "settings_sets.role の名前を低優先度から列挙。後のセットを優先し、Role の直接指定を最優先にする。"},
})
channel_attributes = {
    "type": {"enum": ["category", "text", "announcement", "voice", "stage", "forum", "media"]},
    "name": managed(string), "parent": {"oneOf": [logical_id, ref("clear")]},
    "topic": managed({"type": "string", "maxLength": 1024}, True), "nsfw": managed(boolean),
    "slowmode_seconds": managed(integer), "default_auto_archive_minutes": managed({"enum": [60, 1440, 4320, 10080]}),
    "default_thread_slowmode_seconds": managed(integer), "bitrate": managed(integer),
    "user_limit": managed(integer), "rtc_region": managed(string, True),
    "video_quality": managed({"enum": ["auto", "full"]}),
    "permissions_sync": boolean,
    "overwrites": mapping(ref("overwrite"), r"^(everyone|role:[A-Za-z0-9_-]+|member:[A-Za-z0-9_-]+)$"),
    "tags": mapping(ref("tag")), "require_tag": managed(boolean),
    "default_reaction": managed(string, True), "default_sort_order": managed({"enum": ["latest_activity", "creation_date"]}, True),
    "default_forum_layout": managed({"enum": ["not_set", "list", "gallery"]}),
}
definitions["channel_settings_set"] = obj(channel_attributes)
definitions["channel"] = resource({
    **channel_attributes,
    "settings_sets": {**id_list, "description": "settings_sets.channel の名前を低優先度から列挙。後のセットを優先し、Channel の直接指定を最優先にする。"},
})
definitions["message_set"] = {"oneOf": [
    obj({"channel": logical_id, "body": ref("messages"),
         "ensure": {"const": "present"}}),
    obj({"ensure": {"const": "absent"}}, ["ensure"]),
]}

schema = {
    "$schema": "http://json-schema.org/draft-04/schema#",
    "title": "Discord 管理定義（設計草案・名前付き設定セット）",
    "description": "TOMLを解析したオブジェクト用。設定セットの展開後の必須属性、参照、ID重複、Discord型別制約は別途意味検証が必要。",
    **obj({
        "schema_version": {"const": 1},
        "settings_sets": obj({"channel": mapping(ref("channel_settings_set")), "role": mapping(ref("role_settings_set"))}),
        "roles": mapping(ref("role")), "channels": mapping(ref("channel")),
        "members": mapping(obj({"mode": {"const": "reference"}}, ["mode"])),
        "message_sets": mapping(ref("message_set")),
        "threads": mapping(ref("thread")),
        "order": obj({"roles": id_list, "categories": id_list, "children": mapping(id_list)}),
    }, ["schema_version"]),
    "definitions": definitions,
}
destination = ROOT / "docs/schemas/discord-management.schema.json"


def draft4(value):
    if isinstance(value, dict):
        return {("enum" if key == "const" else key): ([item] if key == "const" else draft4(item))
                for key, item in value.items()}
    if isinstance(value, list):
        return [draft4(item) for item in value]
    return value


schema = draft4(schema)
destination.parent.mkdir(parents=True, exist_ok=True)
destination.write_text(json.dumps(schema, ensure_ascii=False, indent=2) + "\n", encoding="utf-8")
print(destination)
