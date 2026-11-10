#!/usr/bin/env bash
# ============================================================================
# CXCOM macOS 打包脚本：生成可双击、可分发的 CXCOM.app
#
#   ./build_app.sh              release 构建 + 打包 + ad-hoc 签名
#   ./build_app.sh --universal  同时构建 arm64 + x86_64（通用二进制）
#   ./build_app.sh --dmg        额外生成可分发的 dist/CXCOM-<版本>.dmg
#   ./build_app.sh --no-sign    跳过签名（仅本机调试）
#   ./build_app.sh --clean      打包前先 cargo clean
#   ./build_app.sh --open       打包完成后直接打开 App
#   ./build_app.sh --reveal     在访达中定位生成的 .app
#   ./build_app.sh -h|--help    显示帮助
#
# 首次使用需要安装 cargo-bundle：
#   cargo install cargo-bundle
#
# 产物：
#   target/release/CXCOM.app   （与原始二进制同目录，直接双击）
#   dist/CXCOM.app             （分发副本；--dmg 时再生成 dmg）
#
# 注意：target/release/CXCOM 这个纯二进制文件永远不会显示图标（macOS 对
#       无 bundle 的 Unix 可执行文件固定显示 exec 图标），请运行 .app。
# ============================================================================
set -euo pipefail

cd "$(dirname "$0")"

APP_NAME="CXCOM"
ICON_PNG="icon/tu.png"

DO_SIGN=1
DO_OPEN=0
DO_CLEAN=0
DO_DMG=0
DO_REVEAL=0
UNIVERSAL=0

usage() {
    cat <<'EOF'
用法: ./build_app.sh [选项]

  --universal  同时构建 arm64 + x86_64（通用二进制）
  --dmg        额外生成 dist/CXCOM-<版本>.dmg（含“应用程序”快捷方式）
  --no-sign    跳过 ad-hoc 签名
  --clean      打包前 cargo clean
  --open       打包完成后打开 App
  --reveal     在访达中定位生成的 .app
  -h, --help   显示本帮助
EOF
}

while [ $# -gt 0 ]; do
    case "$1" in
        --universal) UNIVERSAL=1 ;;
        --dmg)       DO_DMG=1 ;;
        --no-sign)   DO_SIGN=0 ;;
        --clean)     DO_CLEAN=1 ;;
        --open)      DO_OPEN=1 ;;
        --reveal)    DO_REVEAL=1 ;;
        -h|--help)   usage; exit 0 ;;
        *) echo "未知参数: $1" >&2; usage; exit 1 ;;
    esac
    shift
done

if [ "$(uname -s)" != "Darwin" ]; then
    echo "错误：build_app.sh 只能在 macOS 上运行。" >&2
    exit 1
fi

if ! command -v cargo >/dev/null 2>&1; then
    echo "错误：找不到 cargo，请先安装 Rust：https://rustup.rs" >&2
    exit 1
fi

if ! cargo bundle --help >/dev/null 2>&1; then
    cat >&2 <<'EOF'
错误：缺少 cargo-bundle（macOS .app 打包工具）。
请先安装（注意要加 --locked，否则会解析到不兼容的新依赖，编译 sigstore 时报错）：

    cargo install cargo-bundle --locked

EOF
    exit 1
fi

if [ ! -f "$ICON_PNG" ]; then
    echo "错误：找不到图标 $ICON_PNG" >&2
    exit 1
fi

if command -v sips >/dev/null 2>&1; then
    read -r ICON_W ICON_H <<<"$(sips -g pixelWidth -g pixelHeight "$ICON_PNG" 2>/dev/null \
        | awk '/pixelWidth/{w=$2}/pixelHeight/{h=$2}END{print w" "h}')"
    if [ -n "${ICON_W:-}" ]; then
        echo "图标: $ICON_PNG (${ICON_W}x${ICON_H}, cargo-bundle 会自动转成 .icns)"
        if [ "$ICON_W" -lt 512 ] || [ "$ICON_H" -lt 512 ]; then
            echo "提示：图标小于 512x512，Retina 下会偏糊，建议换成 1024x1024。" >&2
        fi
    fi
fi

if [ "$UNIVERSAL" -eq 1 ]; then
    if ! command -v rustup >/dev/null 2>&1; then
        echo "错误：--universal 需要 rustup 管理 target。" >&2
        exit 1
    fi
    for t in aarch64-apple-darwin x86_64-apple-darwin; do
        if ! rustup target list --installed 2>/dev/null | grep -qx "$t"; then
            echo "错误：缺少 target ${t}，请先运行：" >&2
            echo "    rustup target add aarch64-apple-darwin x86_64-apple-darwin" >&2
            exit 1
        fi
    done
fi

if [ "$DO_CLEAN" -eq 1 ]; then
    echo "cargo clean ..."
    cargo clean
fi

# cargo 的 target 目录可能被 CARGO_TARGET_DIR 或 .cargo/config.toml 改到项目外
# （例如本机把它设成了 /Volumes/CX/Applications/Rust/target），不能写死 target/。
TARGET_DIR=$(cargo metadata --format-version 1 --no-deps 2>/dev/null \
    | sed -n 's/.*"target_directory":"\([^"]*\)".*/\1/p' | head -n 1)
if [ -z "${TARGET_DIR:-}" ] || [ ! -d "$TARGET_DIR" ]; then
    TARGET_DIR="${CARGO_TARGET_DIR:-target}"
fi
echo "target 目录: $TARGET_DIR"

BUNDLE_ARGS=(--release --format osx)
if [ "$UNIVERSAL" -eq 1 ]; then
    BUNDLE_ARGS+=(-t aarch64-apple-darwin -t x86_64-apple-darwin)
    echo "构建通用二进制 (arm64 + x86_64) ..."
else
    echo "构建 release ..."
fi

START=$(date +%s)
cargo bundle "${BUNDLE_ARGS[@]}"
ELAPSED=$(( $(date +%s) - START ))

# cargo-bundle 的产物位置随 target 参数变化（release / <triple> / universal），
# 这里取 target/ 下最新的 CXCOM.app。
APP_PATH=""
while IFS= read -r p; do
    if [ -z "$APP_PATH" ] || [ "$p" -nt "$APP_PATH" ]; then
        APP_PATH="$p"
    fi
done < <(find "$TARGET_DIR" -type d -name "$APP_NAME.app" -path "*/bundle/osx/*" 2>/dev/null)

if [ -z "$APP_PATH" ]; then
    echo "错误：打包结束但没有找到 $APP_NAME.app" >&2
    exit 1
fi

if [ "$DO_SIGN" -eq 1 ]; then
    if command -v codesign >/dev/null 2>&1; then
        echo "ad-hoc 签名 ..."
        if codesign --force --sign - "$APP_PATH" >/dev/null 2>&1 \
            && codesign --verify --strict "$APP_PATH" >/dev/null 2>&1; then
            echo "签名校验通过（ad-hoc，仅本机可信）"
        else
            echo "警告：ad-hoc 签名失败，Apple Silicon 上可能无法启动。" >&2
        fi
    else
        echo "提示：找不到 codesign，跳过签名。"
    fi
fi

PLIST="$APP_PATH/Contents/Info.plist"
PB=/usr/libexec/PlistBuddy
VERSION=$("$PB" -c "Print :CFBundleShortVersionString" "$PLIST" 2>/dev/null || echo "0.0.0")

# 1) 分发副本：dist/CXCOM.app
mkdir -p dist
rm -rf "dist/$APP_NAME.app"
cp -R "$APP_PATH" "dist/$APP_NAME.app"
DIST_APP="dist/$APP_NAME.app"

# 2) 放到原始二进制旁边：target/release/CXCOM.app
#    原始二进制永远显示 exec 图标，把 .app 放这里，双击就不会点错。
SIDE_APP=""
REL_DIR=$(cd "$(dirname "$APP_PATH")/../.." 2>/dev/null && pwd || true)
if [ -n "$REL_DIR" ] && [ "$(basename "$REL_DIR")" = "release" ]; then
    rm -rf "$REL_DIR/$APP_NAME.app"
    cp -R "$APP_PATH" "$REL_DIR/$APP_NAME.app"
    SIDE_APP="$REL_DIR/$APP_NAME.app"
fi

# 3) 可选：生成可分发的 DMG（hdiutil 受限时自动退回 zip）
PACKAGE_PATH=""
PACKAGE_KIND=""
if [ "$DO_DMG" -eq 1 ]; then
    if command -v hdiutil >/dev/null 2>&1; then
        echo "生成 DMG ..."
        STAGE=$(mktemp -d)
        mkdir -p "$STAGE/root"
        cp -R "$DIST_APP" "$STAGE/root/"
        ln -s /Applications "$STAGE/root/Applications"
        DMG_PATH="dist/$APP_NAME-$VERSION.dmg"
        rm -f "$DMG_PATH"
        if hdiutil create -quiet -volname "$APP_NAME" -srcfolder "$STAGE/root" \
            -ov -format UDZO "$DMG_PATH" >/dev/null 2>&1; then
            PACKAGE_PATH="$DMG_PATH"
            PACKAGE_KIND="DMG"
        fi
        rm -rf "$STAGE"
    fi
    if [ -z "$PACKAGE_PATH" ]; then
        echo "提示：hdiutil 不可用（受限环境），改为生成 zip。" >&2
        ZIP_PATH="dist/$APP_NAME-$VERSION.zip"
        rm -f "$ZIP_PATH"
        if command -v ditto >/dev/null 2>&1 \
            && ditto -c -k --sequesterRsrc --keepParent "$DIST_APP" "$ZIP_PATH"; then
            PACKAGE_PATH="$ZIP_PATH"
            PACKAGE_KIND="ZIP"
        else
            echo "警告：DMG / ZIP 都生成失败。" >&2
            rm -f "$ZIP_PATH"
        fi
    fi
fi

IDENT=$("$PB" -c "Print :CFBundleIdentifier" "$PLIST" 2>/dev/null || echo "?")
ICONFILE=$("$PB" -c "Print :CFBundleIconFile" "$PLIST" 2>/dev/null || echo "?")
EXE_ARCH="?"
if command -v lipo >/dev/null 2>&1; then
    EXE_ARCH=$(lipo -archs "$APP_PATH/Contents/MacOS/$APP_NAME" 2>/dev/null || echo "?")
fi

echo
echo "完成："
echo "  $DIST_APP"
if [ -n "$SIDE_APP" ]; then
    echo "  $SIDE_APP   （与二进制同目录，直接双击）"
fi
if [ -n "$PACKAGE_PATH" ]; then
    echo "  $PACKAGE_PATH   （${PACKAGE_KIND}，可分发）"
fi
echo
echo "  version  : $VERSION"
echo "  bundle id: $IDENT"
echo "  icon     : $ICONFILE"
echo "  架构     : $EXE_ARCH"
echo "  大小     : $(du -sh "$DIST_APP" | cut -f1)"
echo "  用时     : ${ELAPSED}s"
echo
echo "提示：cargo 输出目录里的 $APP_NAME 是原始二进制，永远显示 exec 图标；"
echo "      请双击 $APP_NAME.app 运行。"
echo "首次打开若提示“无法验证开发者”，在访达里右键 →「打开」，或执行："
echo "    xattr -dr com.apple.quarantine \"$DIST_APP\""
echo "图标缓存没刷新时执行：killall Dock"

if [ "$DO_REVEAL" -eq 1 ]; then
    open -R "$DIST_APP"
fi
if [ "$DO_OPEN" -eq 1 ]; then
    open "$DIST_APP"
fi
