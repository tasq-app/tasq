# Trailer seed: the showcase list, in the database, Apple connected to a local CalDAV.
cd /home/user/tasq
source docs/videos/setup.sh >/dev/null
export TUXEDO_NO_UPDATE_CHECK=1 TASQ_NO_GOOGLE=1 NO_PROXY=127.0.0.1 no_proxy=127.0.0.1
echo 'caldav_url = http://127.0.0.1:5232/' >> $D/cfg/tasq/config.toml
sed -i 's/^start = .*/start = "list"/' $D/cfg/tasq/config.toml
printf 'http://127.0.0.1:5232/\nyou@icloud.com\nsecret-pass\n' > $D/cfg/tasq/apple-account
chmod 600 $D/cfg/tasq/apple-account
export TASQ_GOOGLE_CLIENT_ID=demo.apps.googleusercontent.com
