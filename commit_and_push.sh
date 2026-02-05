#!/bin/bash

# Dừng ngay nếu có lỗi
set -e

# Kiểm tra đang ở repo git chưa
if ! git rev-parse --is-inside-work-tree >/dev/null 2>&1; then
  echo "❌ Không phải git repository"
  exit 1
fi

# Kiểm tra có thay đổi không
if git diff --quiet && git diff --cached --quiet; then
  echo "⚠️  Không có gì để commit"
  exit 0
fi

# Nhập commit message
read -p "Commit message (để trống dùng mặc định): " msg

if [ -z "$msg" ]; then
  msg="Update"
fi

# Add, commit, push
git add .
git commit -m "$msg"
git push

echo "✅ Commit & push xong"
