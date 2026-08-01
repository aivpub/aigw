#!/bin/bash
# check-i18n-keys.sh — 检查 i18n 翻译键是否在 en.json 和 zh-CN.json 之间对齐
set -euo pipefail

DIR="$(cd "$(dirname "$0")/../crates/aigw-frontend/src/i18n/locales" && pwd)"
EN="$DIR/en.json"
ZH="$DIR/zh-CN.json"

if [ ! -f "$EN" ] || [ ! -f "$ZH" ]; then
  echo "ERROR: translation files not found"
  exit 1
fi

# 使用 Python 递归提取所有键并比较
python3 << 'PYEOF'
import json, sys

def extract_keys(obj, prefix=""):
    keys = set()
    if isinstance(obj, dict):
        for k, v in obj.items():
            path = f"{prefix}.{k}" if prefix else k
            if isinstance(v, dict):
                keys.update(extract_keys(v, path))
            else:
                keys.add(path)
    return keys

try:
    with open(sys.argv[1]) as f:
        en = json.load(f)
    with open(sys.argv[2]) as f:
        zh = json.load(f)
except Exception as e:
    print(f"JSON parse error: {e}")
    sys.exit(1)

en_keys = extract_keys(en)
zh_keys = extract_keys(zh)

missing_in_zh = en_keys - zh_keys
extra_in_zh = zh_keys - en_keys

if missing_in_zh:
    print(f"❌ Missing in zh-CN.json ({len(missing_in_zh)} keys):")
    for k in sorted(missing_in_zh):
        print(f"   {k}")

if extra_in_zh:
    print(f"⚠️  Extra in zh-CN.json ({len(extra_in_zh)} keys):")
    for k in sorted(extra_in_zh):
        print(f"   {k}")

if not missing_in_zh and not extra_in_zh:
    print("✅ All i18n keys aligned between en.json and zh-CN.json")
    sys.exit(0)
else:
    sys.exit(1)
PYEOF "$EN" "$ZH"
