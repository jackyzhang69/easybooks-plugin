---
name: easybooks-record
description: Record income and expense transactions in EasyBooks — quick one-off entries and document/receipt ingestion. Parse the user's file locally (Excel/CSV/PDF/image/email/text) into Entry JSON, dry-run, confirm, then record. Idempotent on source_id so re-imports never double-record. Requires connect-easybooks first. See easybooks-capabilities for the full router and JSON shapes.
when_to_use: |-
  Trigger phrases:
    - "log / record a $X expense for <thing> on <date>"
    - "record this income / payment I received"
    - "here's a receipt / supplier invoice / screenshot / scanned PDF — record it"
    - "import this spreadsheet / bank statement / CSV of expenses"
    - "what categories do I have"
---

# Record EasyBooks transactions

All commands shell out to the bundled `easybooks` CLI. Resolve it once via `easybooks-capabilities/SKILL.md` §B and invoke that exact path; do not rely on ambient `PATH`. All output is JSON on stdout; structured errors go to stderr with a non-zero exit.

**Cardinal rule:** every write goes through the bundled CLI. Never bypass it with a direct backend request or a database write. The agent may parse files locally to extract facts, but the moment data is recorded it goes through `easybooks`.

## Two paths: quick entry vs document import

| Situation | Path |
|---|---|
| User states a single transaction in words ("log a $120 software expense on 2026-05-01") | **Quick entry** — `easybooks expense add` / `income add` |
| User gives a file or pastes content (receipt, supplier invoice, screenshot, PDF, Excel/CSV, statement, email, free text describing purchases) | **Document import** — parse locally → Entry JSON → `tx import-json --dry-run` → confirm → record |

## Quick entry (single, stated in words)

```sh
"$EASYBOOKS_BIN" expense add --amount 120.00 --description "Adobe Creative Cloud" --date 2026-05-01 \
  [--category "Software"] [--classification business|personal] [--notes "..."] [--dry-run]
"$EASYBOOKS_BIN" income add --amount 1500.00 --description "Consulting payment" --date 2026-05-03 \
  [--category "Consulting"] [--classification business|personal] [--dry-run]
```

- `--amount` is a decimal (dollars), e.g. `120.00`. (Batch import JSON uses integer `amount_cents` instead — don't mix the two.)
- `--date` is `YYYY-MM-DD`.
- `--category` is a **name**; the backend resolves it to an id. Never pass a `category_id`. If unsure what categories exist, run `categories list` first.
- `--classification` defaults to `business` when omitted. Ask the user if business-vs-personal is ambiguous — it affects their books.
- For a value you parsed from a document (not stated by the user), prefer the import path so it carries a `--source-id` / `source_id` for idempotency.

## Resolve categories (never guess an id)

```sh
"$EASYBOOKS_BIN" categories list --type expense
"$EASYBOOKS_BIN" categories list --type income
"$EASYBOOKS_BIN" categories list            # both types
```

Use this to map a user's wording ("software", "subscriptions") to a real category name before recording, or to tell the user what categories are available. You pass names to `expense add` / `income add` / Entry JSON; the backend does the name→id resolution.

## Document import: the file-import decision tree (the core path)

The CLI is JSON-first. It does **not** read local file paths. The agent reads the file locally, converts it to Entry JSON, then sends that JSON through the CLI.

| Source material | How the agent parses it locally | Then |
|---|---|---|
| Excel / XLSX / CSV table (statement, expense log) | Read the sheet; one row → one Entry. Map columns to amount / date / description / category. | one batch `tx import-json` |
| PDF (supplier invoice, receipt, bank statement) | Extract text; pull amount, date, vendor, tax. | Entry per charge |
| Image / photo / scan of a receipt | OCR the image; read total, date, merchant. | usually one Entry |
| Email body / forwarded receipt | Read the text; extract amount, date, vendor. | Entry per receipt (Gmail flow → use `easybooks-gmail`) |
| Plain text the user pasted | Parse the described transactions. | Entry per transaction |

### Mandatory sequence

```sh
# 1. Agent parses the user's file(s)/text locally → builds the Entry JSON below.
# 2. Dry-run: validates + echoes resolved rows, writes nothing.
"$EASYBOOKS_BIN" tx import-json --json '<json>' --dry-run
# 3. Show the user the resolved rows (amount, date, category, classification, type). Get confirmation.
# 4. Record for real (same JSON, no --dry-run):
"$EASYBOOKS_BIN" tx import-json --json '<json>'
```

### Entry JSON shape (do not invent fields)

Envelope (the user is identified by the API key, so no owner id is sent):
```json
{
  "source_system": "receipt-drop",
  "entries": [ <Entry>, <Entry>, ... ]
}
```

Each `<Entry>`:
```json
{
  "type": "income|expense",
  "amount_cents": 12000,
  "description": "Adobe Creative Cloud — May",
  "date": "2026-05-01",
  "category_name": "Software",
  "classification": "business",
  "source_type": "receipt|invoice|email|statement",
  "source_id": "stable-unique-id",
  "source_payload": { "vendor": "Adobe", "raw_total": "120.00" }
}
```

- **`amount_cents` is an integer** (cents). $120.00 → `12000`. Do the conversion when you build the JSON.
- **`source_id` is REQUIRED for imports** and must be stable for the same underlying document. Good choices: the supplier invoice number, a hash of `(vendor + date + amount)` for a statement line, the receipt number, or the email/Gmail message id. This is what makes re-imports safe.
- `category_name` is optional; the backend resolves the name to an id. `classification` defaults to `business`.
- Zero-amount rows are skipped server-side.

## Idempotency — read this before recording

Recorded rows are upserted on **`(user_id, source_system, source_id)`**. That means:

- Running the **same** import twice does **not** create duplicates. The second run reports the rows under `existing`, not `created`.
- The output is `{ "created":n, "existing":n, "skipped":n, "processed":n }`. After a re-run you should expect `created: 0` and `existing` equal to the previously created count — **that is success, not a failure**. Do not "retry to fix it".
- Therefore: pick a **deterministic** `source_id`. If you generate a random id each run you defeat idempotency and will double-record. Never do that.
- Keep `source_system` stable per source (e.g. `receipt-drop`, `bank-statement`, `gmail`). The same document recorded under two different `source_system` values WILL appear twice — that's two distinct idempotency keys.

## Quick router (user intent → exact command)

| If the user says… | Run |
|---|---|
| "log a $X expense for `<thing>` on `<date>`" | `"$EASYBOOKS_BIN" expense add --amount <d> --description "<t>" --date <YYYY-MM-DD> [--category "<name>"]` |
| "record $X income / a payment I received on `<date>`" | `"$EASYBOOKS_BIN" income add --amount <d> --description "<t>" --date <YYYY-MM-DD> [--category "<name>"]` |
| "record this receipt / supplier invoice / screenshot / PDF" | parse locally → Entry JSON → `tx import-json --json '<json>' --dry-run` → confirm → rerun without `--dry-run` |
| "import this spreadsheet / CSV / bank statement of expenses" | parse rows locally → batch Entry JSON → `tx import-json --dry-run` → confirm → rerun |
| "what categories do I have" | `"$EASYBOOKS_BIN" categories list [--type income\|expense]` |
| "scan my Gmail for receipts" | hand off to **easybooks-gmail** (uses `gmail record`, source_id = message id) |

## Default behavior & confirmation

- A single quick `expense add` / `income add` where the user stated every field can be run directly.
- For **any document import**, always `--dry-run` first and show the resolved rows. Money accuracy beats a saved round-trip. Confirm amounts and the business/personal classification.
- If classification or category is ambiguous, ask one specific question — don't silently default to `business` on a personal purchase.
- When the CLI returns a structured error with a `hint`, surface it verbatim.

## Governance

- The CLI **defaults to the PROD backend** (`https://easybooks.jackyzhang.app`, the immicore eb-plugin via the eb frontend nginx `/api` proxy); the legacy Node `http://localhost:8080` is no longer the default. Override to test (`https://easybooks-test.jackyzhang.app`) or LAN (`http://192.168.1.98:8310`) via `--base-url`. Because the default is production, recording is a production write and requires an approval artifact (see `easybooks-capabilities` §G). If you'd be writing to production without one, stop and tell the user.
- Recording requires a **read_write** API key. If the CLI returns a scope/permission error, the user's key is read-only — tell them to create a Read & write key in the EasyBooks web app (Settings → API Keys).
- Never print the user's API key; it is masked as `eb_***` and lives only in `~/.easybooks/config.json`.
