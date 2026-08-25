## Shared platform token (host agent — mandatory)

- Canonical durable user credential: `~/.jackyzhang.app/token/user.json` (`jz_` only).
- **One connect for the whole platform.** If this file already exists from any other official plugin, do **not** ask the human to log in again.
- On load, if `user.json` is missing but a legacy on-disk `jz_` exists (`token/user.json`, product-local config token fields), **migrate immediately** by running this product's CLI `whoami`/`doctor`/any authenticated command (load-time migrate is built in). User must feel no re-paste.
- Only when no migratable `jz_` exists: complete connect once using the token-delivery rules below. After that, every registered plugin reuses the same slot.
- Never print or log the raw token. Prefer masked doctor/whoami output.
- Do not create product-local durable token files.

## Token delivery to the host agent (connect) — LOCKED 2026-08-14

The host agent performs connect **for** the human. **Never** tell the human to open a terminal and run login commands themselves.

Accept input in this order:

1. **File containing the token (preferred).** If the human provides a filesystem path (e.g. `~/Desktop/jacky-token.txt`) or an attached/readable file whose contents are a single `jz_…` value (optional surrounding whitespace/newline only):
   - Read the file in the agent tool channel.
   - Pipe the token to the product CLI via stdin only: `login --token-stdin` (or the product's equivalent).
   - Do **not** put the token on argv, in chat echo, in logs, or in screenshots.
   - Confirm success with masked doctor/whoami only.

2. **Plaintext token in chat (allowed, discouraged).** If the human pastes a raw `jz_…` into the conversation:
   - **Warn once** in plain language: prefer a local file path next time so the secret is not retained in chat history; do **not** refuse.
   - Proceed immediately: feed that value to `login --token-stdin` via a non-echoing agent-side pipe (temp file mode 0600 deleted after use is OK; never `echo TOKEN |` in a way that lands in shell history if avoidable).
   - Do not repeat the raw token back in the reply.

3. **No token yet.** Ask the human to send either a **file path** (best) or paste the token. Still do not ask them to run terminal commands.

Hard rules:

- `--token <value>` / argv secrets remain **forbidden** for the CLI.
- Agent may read a user-supplied path and stdin-feed the CLI; that is the supported file path.
- After any successful connect, other plugins must not re-prompt when `user.json` is present.


# EasyBooks invoices

All commands shell out to the bundled `easybooks` CLI. Resolve it once via `the easybooks router/SKILL.md` §B and invoke that exact path; do not rely on ambient `PATH`. All output is JSON on stdout; structured errors go to stderr with a non-zero exit.

**Cardinal rule:** invoice creation and sending go through the bundled CLI only — never a direct backend request or database write. The server computes totals and generates the invoice number; do not compute or invent those yourself.

## Step 1 — resolve the client (never guess a client_id)

Two ways to attach a client to an invoice:
- **Existing client** → find them first and pass `{ "client_id": "<uuid>" }`.
- **New / one-off client** → pass `{ "name": ..., "email"?: ..., "address"?: ..., "phone"?: ... }`. The backend resolves an existing client by `(user_id, email|name)` or creates one.

```sh
"$EASYBOOKS_BIN" clients list                       # all clients
"$EASYBOOKS_BIN" clients find --query "Acme"         # search by name/email
```

- If `clients find` returns exactly one match, use its `client_id`.
- If it returns several, ask the user which one — do not pick arbitrarily.
- If it returns none and the user gave client details, let the create path make the client (pass `name` + `email`). Confirm the spelling/email with the user first if it's a new client.

## Step 2 — build the invoice JSON

```json
{
  "client": { "name": "Acme Co", "email": "ap@acme.co" },
  "issue_date": "2026-05-01",
  "due_date": "2026-05-31",
  "tax_rate": 13,
  "items": [
    { "description": "Consulting — May", "quantity": 10, "unit_price": 150 },
    { "description": "Onboarding setup", "quantity": 1, "unit_price": 500 }
  ],
  "notes": "Net 30. Thank you.",
  "payment_details": "Interac e-transfer to ap@acme.co",
  "source_id": "<optional — see idempotency>"
}
```

Use `{ "client_id": "<uuid>" }` instead of the `client` name block when you resolved an existing client in step 1.

Field rules (do not invent fields):
- `issue_date` / `due_date` are `YYYY-MM-DD`.
- `items[].quantity` and `items[].unit_price` are decimals (dollars). **Do not** send a precomputed line total, subtotal, tax, or grand total — the server computes `subtotal`, `tax_amount`, and `total` from quantity × unit_price and `tax_rate`.
- `tax_rate` is a percentage number (e.g. `13` for 13%). Omit it for a tax-exempt / zero-tax invoice; confirm the rate with the user if unsure (Canadian GST/HST varies by province).
- The server generates the `INV-`-prefixed invoice number; never set one yourself.

## Step 3 — dry-run, confirm, create

```sh
# 3a. Preview: validates client resolution + echoes the server-computed totals, writes nothing.
"$EASYBOOKS_BIN" invoice create --json '<json>' --dry-run
# 3b. Show the user: client, line items, subtotal, tax, total, due date. Confirm.
# 3c. Create for real (same JSON, no --dry-run):
"$EASYBOOKS_BIN" invoice create --json '<json>'
```

Create output:
```json
{ "invoice_id": "<uuid>", "invoice_number": "INV-0042", "total": 2260.00, "created": true }
```

`created: false` means an existing invoice was matched (idempotency hit) rather than a new one made — surface the existing `invoice_number`, don't retry.

### Idempotency

- When you pass `source_id`, the invoice is idempotent on `(user_id, source_id)` — re-running the same create returns the existing invoice (`created:false`) instead of duplicating it. Use a stable `source_id` (e.g. the originating record id) when the create might be retried.
- When `source_id` is omitted, idempotency falls back to `(user_id, invoice_number)`. Since the number is server-generated, a retried create without a `source_id` can produce a second invoice — prefer passing a stable `source_id` for any automated/retryable flow.

## Step 4 — send (emails the client — confirm first)

```sh
"$EASYBOOKS_BIN" invoice send <invoice_id>
```

- This proxies to the backend's existing send endpoint and **emails the invoice/receipt to the client**. Treat it as destructive: **confirm with the user once** ("send invoice INV-0042 to ap@acme.co?") before running.
- Output is passed through from the backend. If it errors (e.g. missing client email), surface the error verbatim and fix the client record rather than retrying blindly.
- Never send before the user has reviewed the totals from step 3.

## List / inspect invoices

```sh
"$EASYBOOKS_BIN" invoices list                  # all invoices
"$EASYBOOKS_BIN" invoices list --status unpaid   # filter by status (e.g. draft, sent, paid, unpaid)
```

Use this for "which invoices are unpaid", "show me last month's invoices", or to find an `invoice_id` to send.

## Quick router (user intent → exact command)

| If the user says… | Run |
|---|---|
| "find client `<name>`" / "list my clients" | `"$EASYBOOKS_BIN" clients find --query "<q>"` / `clients list` |
| "create an invoice for `<client>` for `<items>`" | resolve client → build JSON → `invoice create --json '<json>' --dry-run` → confirm → rerun without `--dry-run` |
| "send invoice `<id>` / email it to the client" | `"$EASYBOOKS_BIN" invoice send <invoice_id>` — **CONFIRM first; it emails the client** |
| "list my invoices / which are unpaid" | `"$EASYBOOKS_BIN" invoices list [--status <s>]` |

## Default behavior

- Always `--dry-run` before a real `invoice create` and show the user the server-computed subtotal / tax / total. Invoices are client-facing money documents; never skip the preview.
- For `invoice send`, confirm once, then send. Don't send without a prior review of totals.
- For ambiguous client matches or an uncertain tax rate, ask one specific question rather than guessing.
- Surface any CLI `hint` verbatim.

## Governance

- The CLI **defaults to the PROD backend** (`https://easybooks.jackyzhang.app`, the immicore eb-plugin via the eb frontend nginx `/api` proxy); the legacy Node `http://localhost:8080` is no longer the default. Override to test (`https://easybooks-test.jackyzhang.app`) or LAN (`http://192.168.1.69:8310`) via `--base-url`. Creating or sending an invoice there is a production mutation: require the explicit current-session authorization named by the platform-vault project card (see `the easybooks router` §G), or stop. `invoice send` emails a real client.
- Creating and sending invoices require write scope. If the CLI returns a scope/permission error, the user's platform token lacks it — have them recheck their Portal token scope. Do not send them to the EasyBooks web app to mint an API key; `eb_live_` keys are retired.
- Never print the user's platform token; show masked identifiers only. It lives in `~/.jackyzhang.app/token/user.json`.
