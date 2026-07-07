#!/bin/bash
set -euo pipefail

# Usage message
usage() {
    cat <<EOF
Usage: rollback.sh --aigw-url <URL> --litellm-url <URL> --aigw-master-key <KEY> [OPTIONS]

Rollback aigw → litellm: export aigw data back to litellm, stop aigw, start litellm.

Required:
  --aigw-url <URL>         Source aigw database URL
  --litellm-url <URL>      Target litellm database URL
  --aigw-master-key <KEY>  AIGW_MASTER_KEY for decryption

Optional:
  --litellm-master-key <KEY>  LITELLM_MASTER_KEY (auto-extracted if omitted)
  --stop-cmd <CMD>            Command to stop aigw (default: "kill \$(pgrep aigw-server)")
  --start-cmd <CMD>           Command to start litellm (default: "docker-compose up -d")
  --health-url <URL>          litellm health check URL (default: "http://localhost:4000/health")
  --dry-run                   Print commands without executing

Example:
  rollback.sh \\
    --aigw-url "postgres://user:pass@host/aigw" \\
    --litellm-url "postgres://user:pass@host/litellm" \\
    --aigw-master-key "sk-aigw-xxx"
EOF
    exit 1
}

# Defaults
STOP_CMD="kill \$(pgrep aigw-server 2>/dev/null) 2>/dev/null || true"
START_CMD="docker-compose up -d"
HEALTH_URL="http://localhost:4000/health"
DRY_RUN=false
AIGW_MASTER_KEY=""
LITELLM_MASTER_KEY=""

while [[ $# -gt 0 ]]; do
    case "$1" in
        --aigw-url) AIGW_URL="$2"; shift 2 ;;
        --litellm-url) LITELLM_URL="$2"; shift 2 ;;
        --aigw-master-key) AIGW_MASTER_KEY="$2"; shift 2 ;;
        --litellm-master-key) LITELLM_MASTER_KEY="$2"; shift 2 ;;
        --stop-cmd) STOP_CMD="$2"; shift 2 ;;
        --start-cmd) START_CMD="$2"; shift 2 ;;
        --health-url) HEALTH_URL="$2"; shift 2 ;;
        --dry-run) DRY_RUN=true; shift ;;
        *) echo "Unknown option: $1"; usage ;;
    esac
done

# Validate required args
if [[ -z "${AIGW_URL:-}" ]] || [[ -z "${LITELLM_URL:-}" ]] || [[ -z "$AIGW_MASTER_KEY" ]]; then
    echo "ERROR: --aigw-url, --litellm-url, and --aigw-master-key are required"
    usage
fi

echo "=== aigw Rollback: aigw → litellm ==="
echo ""

# Step 1: Stop aigw
echo "Step 1: Stopping aigw..."
if $DRY_RUN; then
    echo "  [DRY RUN] $STOP_CMD"
else
    eval "$STOP_CMD"
    echo "  aigw stopped."
fi

# Step 2: Export aigw → litellm
echo "Step 2: Exporting aigw → litellm..."

EXPORT_CMD="aigw-migrate remote-export \\
  --source-url \"$AIGW_URL\" \\
  --target-url \"$LITELLM_URL\" \\
  --source-master-key \"$AIGW_MASTER_KEY\""

if [[ -n "$LITELLM_MASTER_KEY" ]]; then
    EXPORT_CMD="$EXPORT_CMD --target-master-key \"$LITELLM_MASTER_KEY\""
fi

if $DRY_RUN; then
    echo "  [DRY RUN] $EXPORT_CMD"
else
    eval "$EXPORT_CMD" || {
        echo "ERROR: Export failed!"
        exit 1
    }
    echo "  Export complete."
fi

# Step 3: Start litellm
echo "Step 3: Starting litellm..."
if $DRY_RUN; then
    echo "  [DRY RUN] $START_CMD"
else
    eval "$START_CMD"
    echo "  litellm started."
fi

# Step 4: Health check
echo "Step 4: Health check litellm..."
if $DRY_RUN; then
    echo "  [DRY RUN] curl -sf $HEALTH_URL"
else
    sleep 3
    if curl -sf "$HEALTH_URL" > /dev/null 2>&1; then
        echo "  litellm health check PASSED."
    else
        echo "  WARNING: litellm health check FAILED. Check logs."
        exit 1
    fi
fi

echo ""
echo "=== Rollback complete ==="
