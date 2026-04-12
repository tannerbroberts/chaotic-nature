#!/bin/bash
# Re-export the Godot web build and restart the HTTP server.
set -e
PROJECT="/Users/tanner/dev/chaotic-nature"
EXPORTS="$PROJECT/exports"

echo "=== Exporting web build ==="
godot --headless --export-release "index" "$EXPORTS/index.html" --path "$PROJECT" 2>&1 | tail -3

echo "=== Patching secure context check ==="
# Godot's engine always checks window.isSecureContext. Override it for local HTTP testing.
sed -i '' "s|return window\['isSecureContext'\] === true;|return true;|g" "$EXPORTS/index.js"

echo "=== Patching missing secure-context APIs ==="
# Shim AudioWorklet and other APIs that are unavailable over plain HTTP.
sed -i '' 's|<head>|<head><script>if(!window.isSecureContext){if(window.AudioContext){var _ac=AudioContext.prototype;if(!_ac.audioWorklet){_ac.audioWorklet={addModule:function(){return Promise.resolve()}}}}}</script>|' "$EXPORTS/index.html"

echo "=== Killing old server ==="
lsof -ti:8080 | xargs kill -9 2>/dev/null || true
sleep 1

echo "=== Starting HTTP server on port 8080 ==="
cd "$EXPORTS"
python3 -m http.server 8080 &
SERVER_PID=$!

IP=$(ipconfig getifaddr en0)
echo ""
echo "=== Ready ==="
echo "Open on phone: http://$IP:8080"
echo "Server PID: $SERVER_PID"
echo "To stop: kill $SERVER_PID"
wait $SERVER_PID
