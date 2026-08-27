#!/usr/bin/env bash
#
# prism-backup.sh — raw data-dir rsync of both Prism instances to Synology NAS.
#
# Runs on prismsearch01 (192.168.88.212) as user `m` (passwordless sudo).
# Backs up BOTH Prism instances via raw filesystem rsync:
#   - :3080  /var/lib/prismsearch   (primary)
#   - :4080  /var/lib/prismsearch2  (secondary)
#
# Why raw rsync (not prism-cli snapshot export): the v0.6.11 CLI snapshot
# exporter produced empty/truncated archives for collections without a schema
# match in the schemas-dir (agent_messages: 175M on disk → 274-byte export).
# Raw rsync of the tantivy segment files is a correct, complete backup:
# segment files are immutable once written; only meta.json changes on merge.
#
# Incremental via --link-dest: unchanged files are hardlinked to the previous
# backup (BTRFS supports hardlinks), so each daily dir is a full, independent
# snapshot but only changed files consume extra space/transfer.
#
# Retention on NAS: 7 daily + 4 weekly (Sunday). Older deleted.
#
# Usage:
#   /usr/local/sbin/prism-backup.sh            # run now
#   /usr/local/sbin/prism-backup.sh --verify   # run + list result
#
set -euo pipefail

# --- config -----------------------------------------------------------------
NAS_USER="prism"
NAS_HOST="192.168.88.88"
NAS_KEY="/home/m/.ssh/id_ed25519_prism_backup"
NAS_BASE="/volume1/backups/prism"

# (label, data_dir) pairs for each Prism instance
INSTANCES=(
  "3080:/var/lib/prismsearch"
  "4080:/var/lib/prismsearch2"
)

KEEP_DAILY=7     # keep last 7 daily backups
KEEP_WEEKLY=4    # keep last 4 weekly (Sunday) backups

# --- setup ------------------------------------------------------------------
umask 022
DATE="$(date +%Y-%m-%d)"
DAY_OF_WEEK="$(date +%u)"   # 1=Mon ... 7=Sun
IS_WEEKLY=0
[ "$DAY_OF_WEEK" = "7" ] && IS_WEEKLY=1

SSH_OPTS="-i ${NAS_KEY} -o IdentitiesOnly=yes -o StrictHostKeyChecking=accept-new -o UserKnownHostsFile=/home/m/.ssh/known_hosts -o BatchMode=yes -o ConnectTimeout=15"
RSYNC_SSH="ssh ${SSH_OPTS}"
NAS="${NAS_USER}@${NAS_HOST}"

log()  { printf '[%s] %s\n' "$(date +%H:%M:%S)" "$*" >&2; }
die()  { log "ERROR: $*"; exit "${2:-1}"; }

log "prism-backup start — date=${DATE} weekly=${IS_WEEKLY}"

# --- 0. ensure NAS base dirs exist ------------------------------------------
ssh $SSH_OPTS "$NAS" \
  "mkdir -p ${NAS_BASE}/daily ${NAS_BASE}/weekly" \
  || die "cannot reach NAS or create base dirs" 2

# find previous daily backup (for --link-dest), most recent excluding today
PREV_DAILY="$(ssh $SSH_OPTS "$NAS" \
  "ls -1 ${NAS_BASE}/daily 2>/dev/null | grep -v '${DATE}' | sort -r | head -1" || true)"
log "link-dest previous: ${PREV_DAILY:-(none, full copy)}"

# --- 1. rsync each instance -------------------------------------------------
backup_instance() {
  local label="$1" data_dir="$2" kind="$3"  # kind = daily|weekly
  local dest="${NAS_BASE}/${kind}/${DATE}/${label}"
  local src="${data_dir}/"
  local link_arg=""

  # link-dest only for daily, pointing at previous daily's same label
  if [ "$kind" = "daily" ] && [ -n "$PREV_DAILY" ]; then
    local prev="${NAS_BASE}/daily/${PREV_DAILY}/${label}"
    if ssh $SSH_OPTS "$NAS" "test -d ${prev}"; then
      link_arg="--link-dest=${prev}"
    fi
  elif [ "$kind" = "weekly" ]; then
    # weekly links to today's daily if it exists (same data, no re-transfer)
    local today_daily="${NAS_BASE}/daily/${DATE}/${label}"
    if ssh $SSH_OPTS "$NAS" "test -d ${today_daily}"; then
      link_arg="--link-dest=${today_daily}"
    fi
  fi

  # pre-create destination parent (rsync needs the leaf parent to exist)
  ssh $SSH_OPTS "$NAS" "mkdir -p ${dest}" || die "mkdir dest failed" 2

  log "  [${label}] ${kind} rsync ${src} → ${dest} ${link_arg:+(link-dest)}"
  # sudo locally to read prism-owned data-dir; rsync over ssh as NAS user.
  # Paths have no spaces, so minimal quoting to avoid rsync parsing issues.
  if ! sudo rsync -a --delete --stats $link_arg \
       -e "$RSYNC_SSH" \
       "$src" "${NAS}:${dest}/" >&2; then
    die "rsync failed for [${label}] ${kind}" 2
  fi

  # also back up the instance config + schemas + log (small, top-level files)
  for extra in prism.toml schemas prism.log; do
    if sudo test -e "${data_dir}/${extra}"; then
      sudo rsync -a $link_arg \
        -e "$RSYNC_SSH" \
        "${data_dir}/${extra}" "${NAS}:${dest}/" >/dev/null 2>&1 || true
    fi
  done
}

for inst in "${INSTANCES[@]}"; do
  label="${inst%%:*}"
  data_dir="${inst#*:}"
  backup_instance "$label" "$data_dir" "daily"
done

# weekly copy (hardlinked to today's daily → ~zero extra space)
if [ "$IS_WEEKLY" = "1" ]; then
  for inst in "${INSTANCES[@]}"; do
    label="${inst%%:*}"
    data_dir="${inst#*:}"
    backup_instance "$label" "$data_dir" "weekly"
  done
fi

# write a manifest for restore traceability
ssh $SSH_OPTS "$NAS" "cat > '${NAS_BASE}/daily/${DATE}/MANIFEST.txt'" <<EOF
prism-backup ${DATE}
generated: $(date -Iseconds)
host: $(hostname)
weekly: ${IS_WEEKLY}
link-dest: ${PREV_DAILY:-none}
instances:
$(for inst in "${INSTANCES[@]}"; do
    label="${inst%%:*}"; data_dir="${inst#*:}"
    printf '  %s from %s\n' "$label" "$data_dir"
  done)
method: raw rsync of data-dir (tantivy/vector/graph segment files)
restore: rsync dest back to data-dir, restart prism-server (or use attach)
EOF

# --- 2. retention cleanup on NAS --------------------------------------------
log "retention cleanup (daily>${KEEP_DAILY}, weekly>${KEEP_WEEKLY})"
ssh $SSH_OPTS "$NAS" bash -s <<EOF
set +e
cd ${NAS_BASE}/daily 2>/dev/null
ls -1 2>/dev/null | sort -r | tail -n +$((KEEP_DAILY+1)) | while read d; do
  [ -n "\$d" ] && rm -rf "\$d" && echo "  deleted daily/\$d"
done
cd ${NAS_BASE}/weekly 2>/dev/null
ls -1 2>/dev/null | sort -r | tail -n +$((KEEP_WEEKLY+1)) | while read d; do
  [ -n "\$d" ] && rm -rf "\$d" && echo "  deleted weekly/\$d"
done
exit 0
EOF
log "  retention done"

# --- 3. optional verify -----------------------------------------------------
if [ "${1:-}" = "--verify" ]; then
  log "verify — NAS contents"
  ssh $SSH_OPTS "$NAS" bash -s <<EOF
echo "=== daily (newest 5) ==="
ls -1 ${NAS_BASE}/daily 2>/dev/null | sort -r | head -5
echo "=== today sizes ==="
du -sh ${NAS_BASE}/daily/${DATE}/* 2>/dev/null
echo "=== today file counts ==="
for l in 3080 4080; do
  d=${NAS_BASE}/daily/${DATE}/\$l
  [ -d "\$d" ] && echo "  \$l: \$(find "\$d" -type f | wc -l) files"
done
echo "=== disk ==="
df -h /volume1 | tail -1
echo "=== weekly ==="
ls -1 ${NAS_BASE}/weekly 2>/dev/null | sort -r | head -5
EOF
fi

log "✓ done"
