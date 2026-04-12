#!/bin/bash
# Re-export the Godot web build and restart the game server.
set -e
PROJECT="/Users/tanner/dev/chaotic-nature"
EXPORTS="$PROJECT/exports"
SERVER="$PROJECT/server"

echo "=== Exporting web build ==="
godot --headless --export-release "index" "$EXPORTS/index.html" --path "$PROJECT" 2>&1 | tail -3

echo "=== Killing old server ==="
lsof -ti:9001 | xargs kill -9 2>/dev/null || true
sleep 1

echo "=== Starting game server on port 9001 ==="
node "$SERVER/game_server.js" 9001 &
GAME_PID=$!

echo ""
echo "=== Ready ==="
echo "Game server PID: $GAME_PID"
echo "To stop: kill $GAME_PID"

cleanup() { kill $GAME_PID 2>/dev/null; }
trap cleanup EXIT INT TERM
wait
