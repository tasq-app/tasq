# tasq-relay on Dokploy

`tasq-relay` is the sync server: it keeps each account's changes, sealed
on the devices, and hands them out. It can't read them. One small binary,
one SQLite file, plain HTTP on port 8080 behind your HTTPS.

## Deploy

1. **Create the application.** In Dokploy: *Projects* → your project →
   *Create Service* → *Application*, named `tasq-relay`.
2. **Source:** *GitHub*, repository `tasq-app/tasq`, branch `main`.
3. **Build:** *Build Type* → **Dockerfile**:
   - *Docker File*: `deploy/relay/Dockerfile`
   - *Docker Context Path*: `.`
4. **Storage:** *Advanced* → *Volumes* → *Add Volume*:
   - Volume mount, name `tasq-relay-data`, mount path `/data`.
5. **Domain:** *Domains* → *Add Domain*:
   - Host: `sync.example.com` (a DNS `A` record pointing at the server).
   - Path `/`, container port **8080**, *HTTPS* on, certificate
     *Let's Encrypt*.
6. **Deploy.** When it's up, `https://sync.example.com/health` answers
   `"ok"`.

With *Autodeploy* on, every push to `main` rebuilds it.

## Use it

```sh
tasq sync login --server https://sync.example.com you@example.com
tasq sync          # any time: send and receive
```

The same command, with the same email and password, on every computer.

## What the server knows

The account name (a hash of the email, not the email), a hash of the
key the devices sign in with, and for each batch of changes: which device
sent it, when, and how big it is. The changes themselves are encrypted on
the devices with a key made from the password, which never reaches the
server. Back up the `/data` volume like any other; without the password
it's unreadable.

## Claude (MCP)

The same server answers Claude as a remote MCP server at
`https://sync.example.com/mcp`, with OAuth sign-in at `/oauth/*`. It
works out the public address from the request (`X-Forwarded-Host` /
`Host`, HTTPS); set `TASQ_RELAY_PUBLIC_URL=https://sync.example.com` if
your proxy hides it. Only accounts whose owner connects Claude are ever
read, and only while answering it.
