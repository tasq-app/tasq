# Trailer seed: the showcase list, in the database, Apple connected to a local CalDAV.
cd /home/user/tasq
source docs/videos/setup.sh >/dev/null
export TUXEDO_NO_UPDATE_CHECK=1 TASQ_NO_GOOGLE=1 NO_PROXY=127.0.0.1 no_proxy=127.0.0.1
echo 'caldav_url = http://127.0.0.1:5232/' >> $D/cfg/tasq/config.toml
sed -i 's/^start = .*/start = "list"/' $D/cfg/tasq/config.toml
printf 'http://127.0.0.1:5232/\nyou@icloud.com\nsecret-pass\n' > $D/cfg/tasq/apple-account
chmod 600 $D/cfg/tasq/apple-account
export TASQ_GOOGLE_CLIENT_ID=demo.apps.googleusercontent.com
# Signed in to sync, on a relay of its own for this take (a shared one
# would hand each take the last one's tasks). The demo list goes in first.
$TASQ_BIN ls >/dev/null 2>&1
pkill -f "tasq-relay --addr 127.0.0.1:18095" 2>/dev/null
rm -f /tmp/claude-0/-home-user-tuxedo/062bdb26-4f75-521d-9ea8-6c98b8b80bd9/scratchpad/relay-trailer.db*
(nohup /home/user/tasq/target/release/tasq-relay --addr 127.0.0.1:18095 --db /tmp/claude-0/-home-user-tuxedo/062bdb26-4f75-521d-9ea8-6c98b8b80bd9/scratchpad/relay-trailer.db >/dev/null 2>&1 &)
sleep 0.5
export TASQ_SYNC_URL=http://127.0.0.1:18095
TASQ_SYNC_PASSWORD='trailer password' $TASQ_BIN sync login you@example.com >/dev/null 2>&1
