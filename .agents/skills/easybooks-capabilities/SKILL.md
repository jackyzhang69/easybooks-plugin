---
name: easybooks-capabilities
description: READ THIS FIRST. One-page consumption contract for AI agents using EasyBooks. Tells you exactly which EasyBooks skill / command to call for any user intent (record an expense/income, drop a receipt/invoice file, create or send an invoice, scan Gmail for receipts, list categories/clients/invoices, health/doctor, connect), how to resolve the bundled binary, what runs locally vs through the backend, and the production governance gate. Read this before guessing parameters or trying commands.
when_to_use: |-
  Load on plugin start; reload whenever a user asks anything EasyBooks-related.
  Trigger phrases: "record this receipt", "log a $X expense/income", "I dropped a file / invoice / screenshot",
  "create an invoice", "send invoice X", "scan my Gmail for receipts", "list my categories/clients/invoices",
  "is EasyBooks healthy / which backend am I on", "connect EasyBooks".
---

# EasyBooks plugin — agent consumption contract

**Read this once on plugin load and reload it whenever a user asks anything EasyBooks-related.** It tells you which skill / subcommand to call for each user intent, what to never guess, and where the production safety gate is.

EasyBooks is a self-employed (Canadian) finance app: income/expense transactions, categories, clients, and invoices. This plugin lets an agent record bookkeeping through one bundled CLI instead of touching the database or backend directly.

## 0. Non-negotiable operating rules

1. **All EasyBooks system operations go through the bundled CLI.** Reads (categories / clients / invoices), recording income/expense, importing parsed documents, creating invoices, and sending invoices must use `<easybooks> ...`. Do **not** call EasyBooks backend endpoints directly and do **not** write to Supabase / any database directly. The CLI is the only boundary.
2. **Local file parsing is allowed only before the CLI boundary.** The agent may read Excel, CSV, PDF, image (receipt photo / scan), email, or plain text locally to extract facts. The moment data is recorded, listed, or mutated, call the CLI.
3. **Never guess ids.** Resolve category names and client names to ids by recording with names (the backend resolves them) or by listing first (`categories list`, `clients find`, `invoices list`). Do not invent a `category_id` or `client_id`.
4. **Idempotency is mandatory for any recorded document.** Every parsed receipt / invoice / email row carries a stable `source_id`. Re-running the same import must not double-record. For Gmail, `source_id` is the Gmail message id (see `easybooks-gmail`).
5. **Production is the default; writes are gated.** The CLI defaults to the PROD backend (`https://easybooks.jackyzhang.app`, the immicore eb-plugin via the eb frontend nginx `/api` proxy). Override to test (`https://easybooks-test.jackyzhang.app`) or LAN (`http://192.168.1.98:8310`) via `--base-url`. Because the default is production, any production write requires an approval artifact — see §G. Warn the user before any production write.

## Agent quick router — TOP 20 LINES (read this first)

User said this → call this exact command (binary resolution: §B; file-drop decision tree: §C; full per-skill detail in the linked skill):

| User intent | Command (one-hop preferred) | Skill |
|---|---|---|
| "record / log a $X **expense** for `<thing>` on `<date>`" | `easybooks expense add --amount <d> --description "<t>" --date <YYYY-MM-DD> [--category <name>] [--classification business\|personal]` | easybooks-record |
| "record / log a $X **income** / payment received on `<date>`" | `easybooks income add --amount <d> --description "<t>" --date <YYYY-MM-DD> [--category <name>]` | easybooks-record |
| "here's a **receipt / invoice file** (PDF / image / Excel / CSV / email / text) — record it" | parse locally → build Entry JSON (§2) → `easybooks tx import-json --json '<json>' --dry-run` → show user → rerun without `--dry-run` | easybooks-record |
| "record **several** transactions / a statement / a spreadsheet of expenses" | parse locally → batch Entry JSON → `easybooks tx import-json --json '<json>' --dry-run` → confirm → rerun | easybooks-record |
| "**create an invoice** for `<client>` for `<items>`" | resolve client → build invoice JSON (§2) → `easybooks invoice create --json '<json>' --dry-run` → confirm → rerun | easybooks-invoice |
| "**send** invoice `<id>` / email the invoice/receipt" | `easybooks invoice send <invoice_id>` (CONFIRM first — it emails the client) | easybooks-invoice |
| "**scan my Gmail** for receipts / invoices and record them" | read candidates via Gmail MCP → extract to Entry JSON with `source_id` = Gmail message id → `easybooks gmail record --json '<json>' --dry-run` → confirm → rerun | easybooks-gmail |
| "list my **categories**" | `easybooks categories list [--type income\|expense]` | easybooks-record |
| "list / find my **clients**" | `easybooks clients list` or `easybooks clients find --query "<q>"` | easybooks-invoice |
| "list my **invoices** [that are unpaid/draft]" | `easybooks invoices list [--status <s>]` | easybooks-invoice |
| "is EasyBooks **healthy** / which backend am I on / token still valid" | `easybooks doctor --json` (local config + backend round-trip + version) | this file |
| "**connect** EasyBooks / save my API key / set it up" | `connect-easybooks` skill → `easybooks login --token eb_*** --base-url <url>` | connect-easybooks |
| "EasyBooks **out of date**?" | `easybooks doctor --json --no-fetch --check-upgrade` | connect-easybooks |

Routing detail below is supplementary — start with this table.

## §B. Resolving the `easybooks` binary

The plugin ships a Rust CLI binary that is NOT placed on `PATH` automatically by either Codex or Claude Code. Throughout these skills, `<easybooks>` or the literal token `easybooks` mean **"the bundled binary at this resolved path"**, not a `PATH` lookup.

**Resolution order (use the first that resolves to an existing executable):**

1. **`$EASYBOOKS_BIN`** — explicit override. Honor if set.
2. **Claude Code plugin dir** — `$CLAUDE_PLUGIN_ROOT/bin/<platform>/easybooks` (Claude Code sets `CLAUDE_PLUGIN_ROOT` when invoking a plugin's skill).
3. **Codex plugin cache** — `$HOME/.codex/plugins/cache/jacky-plugins/easybooks-cli/<highest-version>/bin/<platform>/easybooks` where `<highest-version>` is the highest version dir present and `<platform>` matches the OS/arch.
4. **`command -v easybooks`** — if the user has installed it on PATH manually (last resort).

`<platform>` ∈ `darwin-arm64`, `darwin-x64`, `linux-x64`, `win32-x64` (binary is `easybooks.exe` on `win32-x64`).

**Portable resolver (bash; works on darwin / linux; for Windows agents use the PowerShell variant below):**

```bash
# Detect platform → cache subdir name used by both codex and claude.
case "$(uname -s)-$(uname -m)" in
  Darwin-arm64)  PLAT=darwin-arm64 ;;
  Darwin-x86_64) PLAT=darwin-x64 ;;
  Linux-x86_64)  PLAT=linux-x64 ;;
  *) echo "unsupported platform: $(uname -s)-$(uname -m)" >&2; return 1 2>/dev/null || exit 1 ;;
esac

# Walk a search list in priority order; pick first existing executable.
EASYBOOKS_BIN_RESOLVED=""
_cand_paths=(
  "${EASYBOOKS_BIN:-}"
  "${CLAUDE_PLUGIN_ROOT:+$CLAUDE_PLUGIN_ROOT/bin/$PLAT/easybooks}"
)
# Codex cache may have several version dirs; agent picks the *highest* one.
# Use python sort (universally available) — POSIX `sort -V` is not portable.
_codex_root="$HOME/.codex/plugins/cache/jacky-plugins/easybooks-cli"
if [ -d "$_codex_root" ]; then
  _latest_codex=$(python3 - "$_codex_root" <<'PY' 2>/dev/null
import os, sys
root = sys.argv[1]
def keyfn(d):
    try:    return tuple(int(x) for x in d.split('.'))
    except: return (-1,)
dirs = [d for d in os.listdir(root) if os.path.isdir(os.path.join(root, d))]
dirs.sort(key=keyfn)
print(os.path.join(root, dirs[-1]) if dirs else "", end="")
PY
)
  [ -n "$_latest_codex" ] && _cand_paths+=("$_latest_codex/bin/$PLAT/easybooks")
fi
_cand_paths+=("$(command -v easybooks 2>/dev/null)")

for _p in "${_cand_paths[@]}"; do
  if [ -n "$_p" ] && [ -x "$_p" ]; then EASYBOOKS_BIN_RESOLVED="$_p"; break; fi
done

if [ -z "$EASYBOOKS_BIN_RESOLVED" ]; then
  echo "EasyBooks CLI not found on this host. Install the easybooks-cli plugin first." >&2
  return 1 2>/dev/null || exit 1
fi
export EASYBOOKS_BIN="$EASYBOOKS_BIN_RESOLVED"
"$EASYBOOKS_BIN" --help >/dev/null || { echo "EasyBooks CLI at $EASYBOOKS_BIN is not runnable" >&2; return 1 2>/dev/null || exit 1; }
```

**Windows PowerShell variant** (codex on Windows installs to `$env:USERPROFILE\.codex\...`):

```powershell
$plat = "win32-x64"
$cands = @($env:EASYBOOKS_BIN)
if ($env:CLAUDE_PLUGIN_ROOT) { $cands += "$env:CLAUDE_PLUGIN_ROOT\bin\$plat\easybooks.exe" }
$codexRoot = "$env:USERPROFILE\.codex\plugins\cache\jacky-plugins\easybooks-cli"
if (Test-Path $codexRoot) {
  $latest = Get-ChildItem $codexRoot -Directory | Where-Object { $_.Name -match '^\d+(\.\d+){1,3}$' } | Sort-Object { [version]$_.Name } | Select-Object -Last 1
  if ($latest) { $cands += "$($latest.FullName)\bin\$plat\easybooks.exe" }
}
$cands += (Get-Command easybooks -ErrorAction SilentlyContinue).Source
$env:EASYBOOKS_BIN = $cands | Where-Object { $_ -and (Test-Path $_) } | Select-Object -First 1
```

Once `$EASYBOOKS_BIN` is set, **every command in this doc and every other EasyBooks skill** that starts with the bare token `easybooks` should be invoked as `"$EASYBOOKS_BIN"` (bash) / `& $env:EASYBOOKS_BIN` (PowerShell). The bare `easybooks` token is shorthand throughout these docs; the resolution rule applies uniformly.

**Trust-boundary note**: the `command -v easybooks` fallback trusts the ambient PATH. Prefer the explicit cache paths above when both are available; PATH lookup is the last resort, not the canonical answer.

**Why this matters**: writing `easybooks <subcommand>` assuming PATH is set silently fails in Codex (the binary lives in cache, not PATH). Resolve once at session start; don't waste tool calls hunting for it.

## §C. File-drop / paste decision tree (the core "record this document" path)

Use this whenever the user gives you a file or pastes content: receipt photo, scanned PDF, supplier invoice PDF, Excel/CSV export, bank/credit-card statement, an email, or just plain text describing a purchase.

1. **Agent parses the source locally.** Read the Excel/CSV table, OCR the image, extract PDF text, read the email body — whatever it takes to get the facts. The CLI does **not** read local file paths; you do.
2. **Map each line to an Entry JSON object** (shape in §2). One receipt usually = one Entry; a statement / spreadsheet = many Entries in one batch.
3. **Assign a stable `source_id` per entry** so re-imports are idempotent (invoice number, statement line hash, Gmail message id, etc.). Never omit it for document imports.
4. **Dry-run first:** `easybooks tx import-json --json '<json>' --dry-run`. This validates + echoes the resolved rows without writing.
5. **Show the user the resolved rows** (amount, date, category, classification, type). Get confirmation, especially for amounts and business-vs-personal classification.
6. **Record for real:** rerun the same command **without** `--dry-run`. Output is `{ "created":n, "existing":n, "skipped":n, "processed":n }` — `existing` > 0 means idempotency already had those rows; that is success, not an error.

Single quick one-off (no file)? Use `easybooks income add` / `easybooks expense add` directly instead of building batch JSON. Full detail: `easybooks-record`.

## 2. Data shapes (do not invent fields)

**Entry** (used by `tx import-json` and `gmail record`):

```json
{
  "type": "income|expense",
  "amount_cents": 12000,
  "description": "Adobe Creative Cloud",
  "date": "2026-05-01",
  "category_name": "Software",          // optional; backend resolves to id
  "classification": "business|personal", // optional; defaults business
  "source_type": "receipt|invoice|email|statement", // optional
  "source_id": "stable-unique-id",      // REQUIRED for idempotency on imports
  "source_payload": { }                  // optional raw extracted blob
}
```

`tx import-json` / `gmail record` JSON envelope (the user is identified by the API key, so no owner id):
```json
{ "source_system": "gmail|receipt-drop|...", "entries": [ <Entry>... ] }
```

**Invoice create** JSON (see `easybooks-invoice`):
```json
{
  "client": { "name": "Acme Co", "email": "ap@acme.co" },   // or { "client_id": "<uuid>" }
  "issue_date": "2026-05-01", "due_date": "2026-05-31",
  "tax_rate": 13,
  "items": [ { "description": "Consulting", "quantity": 10, "unit_price": 150 } ],
  "notes": "Net 30", "payment_details": "...", "source_id": "<optional>"
}
```
Server computes subtotal / tax / total and generates the `INV-` invoice number. Amounts in invoice items are decimals (dollars); transaction Entries use integer `amount_cents`.

## 3. Skill router by user intent

| User says (any phrasing) | Skill | Entry point |
|---|---|---|
| "connect / set up EasyBooks / save my key" | **connect-easybooks** | `easybooks login` then `whoami` / `doctor` |
| "log an expense / income", "record this receipt / file / image / PDF / statement" | **easybooks-record** | `expense add` / `income add` / `tx import-json` |
| "create an invoice", "send invoice X", "list my clients / invoices" | **easybooks-invoice** | `invoice create` / `invoice send` / `clients` / `invoices list` |
| "scan my Gmail for receipts / invoices and record them" | **easybooks-gmail** | Gmail MCP read → `gmail record` |
| "is my plugin healthy / which backend am I on" | this file | `easybooks doctor --json` |

## 4. Complete CLI surface by responsibility

| Responsibility | Commands |
|---|---|
| Connect / health | `login`, `whoami`, `doctor` |
| Reads (resolve names → ids) | `categories list [--type income\|expense]`, `clients list`, `clients find --query <q>`, `invoices list [--status <s>]` |
| Record transactions | `income add ...`, `expense add ...`, `tx import-json --json '<json>' [--dry-run]` |
| Invoices | `invoice create --json '<json>' [--dry-run]`, `invoice send <invoice_id>` |
| Gmail (v1) | `gmail record --json '<json>' [--dry-run]` (alias of `tx import-json`, `source_system` defaults to `gmail`), `gmail sync` (v1 stub) |

Treat `<easybooks> --help` as runtime truth when docs and code drift.

## 5. Execution mode boundary — local vs backend

| Step | Where it runs | Network |
|---|---|---|
| Parsing files / OCR / reading Excel-CSV-PDF-email | **Local (agent)** | none |
| Reading Gmail candidate messages | **Local (Gmail MCP)** | the MCP's own |
| `categories/clients/invoices list/find`, `whoami`, all records, `invoice create/send`, `gmail record` | **Backend** call (HTTP to the configured base-url) | required |
| `doctor --no-fetch` | **Local** config read | none |
| `doctor` (default) / `whoami` | **Backend** round-trip | required |

The CLI talks to the EasyBooks backend integration endpoints under `/api/integrations/...`. The agent never hits those endpoints itself.

## 6. Default execution behavior

- If intent is unambiguous AND the operation is **not** a money mutation that needs review (a single tiny `expense add` with all fields known), you may run it directly.
- For **document imports and invoices**, always `--dry-run` first and show the resolved rows/totals before the real write. Money accuracy matters more than a round-trip.
- For **`invoice send`** (emails a client) treat it as destructive: confirm once with the user, then run.
- For **ambiguous intent** (which category? business or personal? which client?), ask one specific clarifying question rather than guessing.
- When the CLI returns a structured error with a `hint`, surface it verbatim — the CLI is the source of truth for what to try next.

### 6.1 Parallelize reads, serialize the config write

The CLI is stateless per invocation. Run independent **reads** in parallel — e.g. `categories list` + `clients list` + `invoices list` when staging an invoice. The only thing that is **serial** is `login` (it writes `~/.easybooks/config.json`). Recording / invoice writes are independent across distinct `source_id`s, but prefer a single batch `tx import-json` over many parallel `expense add` calls so idempotency and the created/existing counts stay coherent.

## §G. Governance — production gate (REQUIRED, surface to user)

- EasyBooks is **not** a normalized platform-vault project. The closest precedent is the audit→EasyBooks integration, whose **production** writes/deploys require approval artifacts.
- The CLI **defaults to the PROD backend** (`https://easybooks.jackyzhang.app`) — the immicore Go eb-plugin reached via the eb frontend domain's nginx `/api` proxy. The legacy Node backend on `http://localhost:8080` is no longer the default. For non-production work, override to **test** (`https://easybooks-test.jackyzhang.app`) or **LAN** (`http://192.168.1.98:8310`) via `--base-url`.
- Because the default is production, **any write is a production write** and is gated. Before any command would write to production EasyBooks, you must have an approval artifact. If the user would write to production without one: **stop and tell them an approval artifact is required**; do not proceed.
- The user's API key is a secret. Never print or log it. It lives only in `~/.easybooks/config.json` (CLI). Mask any value as `eb_***`.

## 8. Token & secret rules

- **Never log the API key value.** Mask any `eb_*` value as `eb_***` in any output you show the user.
- The key is the user's personal EasyBooks API key (`eb_live_...`, scope `read` / `read_write`), minted in the web app and sent as `Authorization: Bearer`. It both authenticates and identifies the user.
- The key lives only in `~/.easybooks/config.json` (or `%USERPROFILE%\.easybooks\config.json`). Captured once by `connect-easybooks`.
- Do not write the key anywhere else, do not include it in example commands, do not echo it back.
- Recording, creating invoices, and sending require a **read_write** key; read commands need **read**. A scope/permission error from the CLI means the key is read-only — tell the user to create a Read & write key in the web app.
