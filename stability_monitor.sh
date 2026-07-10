#!/bin/bash
# Stability test monitor - logs gateway stats every 60s
# Fixed: memory parsing for multiple processes, supports recent compression rate
LOG_FILE="E:/ai/rust-mmo-gate/stability_monitor.log"
DURATION=7200  # 2 hours in seconds
INTERVAL=60

echo "=== Stability Monitor Started ===" > "$LOG_FILE"
echo "Start time: $(date)" >> "$LOG_FILE"
echo "Format: timestamp | elapsed | health | mem_bytes | merge_stats" >> "$LOG_FILE"
echo "---" >> "$LOG_FILE"

START=$(date +%s)
while true; do
    NOW=$(date +%s)
    ELAPSED=$((NOW - START))
    if [ $ELAPSED -ge $DURATION ]; then
        echo "---" >> "$LOG_FILE"
        echo "End time: $(date)" >> "$LOG_FILE"
        echo "Total duration: ${ELAPSED}s" >> "$LOG_FILE"
        break
    fi

    # Get gateway health
    HEALTH=$(curl -s http://127.0.0.1:9090/health 2>/dev/null)
    # Get merge stats (includes recent compression rate)
    MERGE=$(curl -s http://127.0.0.1:9090/merge_stats 2>/dev/null)
    # Get gateway process memory - sum all rust-mmo-gate processes
    MEM=$(powershell.exe -NoProfile -Command "
        \$procs = Get-Process -Name 'rust-mmo-gate' -ErrorAction SilentlyContinue
        if (\$procs) { (\$procs | Measure-Object -Property WorkingSet64 -Sum).Sum } else { 0 }
    " 2>/dev/null | tr -d '\r\n ')

    echo "$(date '+%Y-%m-%d %H:%M:%S') | elapsed=${ELAPSED}s | health=${HEALTH} | mem=${MEM} | merge=${MERGE}" >> "$LOG_FILE"

    sleep $INTERVAL
done
