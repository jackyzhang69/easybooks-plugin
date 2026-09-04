# Command router and CLI paths (coverage reference)

Not for first-session orientation. Host agents discover the live surface via
`"$EASYBOOKS_BIN" commands --json`. Binary resolution: SKILL §B; file-drop tree: §C.

## Agent quick router

| User intent | Command (one-hop preferred) | Skill |
|---|---|---|
| "record / log a $X **expense** for `<thing>` on `<date>`" | `easybooks expense add --amount <d> --description "<t>" --date <YYYY-MM-DD> [--category <name>] [--classification business\|personal]` | references/record.md |
| "record / log a $X **income** / payment received on `<date>`" | `easybooks income add --amount <d> --description "<t>" --date <YYYY-MM-DD> [--category <name>]` | references/record.md |
| "here's a **receipt / invoice file** (PDF / image / Excel / CSV / email / text) — record it" | parse locally → build Entry JSON (§2) → `easybooks tx import-json --json '<json>' --dry-run` → show user → rerun without `--dry-run` | references/record.md |
| "record **several** transactions / a statement / a spreadsheet of expenses" | parse locally → batch Entry JSON → `easybooks tx import-json --json '<json>' --dry-run` → confirm → rerun | references/record.md |
| "this is **personal / mixed**, not business" / "**fix the classification**" | `easybooks tx reclassify <id> --class business\|mixed\|personal [--learn]` (`--learn` remembers the sender) | references/record.md |
| "**attach** the receipt / PDF to this transaction" | `easybooks tx attach-receipt <id> --file <path>` (reads locally, <=10MB, base64; prints `receipt_url`) | references/record.md |
| "**create an invoice** for `<client>` for `<items>`" | resolve client → build invoice JSON (§2) → `easybooks invoice create --json '<json>' --dry-run` → confirm → rerun | references/invoice.md |
| "**send** invoice `<id>` / email the invoice/receipt" | `easybooks invoice send <invoice_id>` (CONFIRM first — it emails the client) | references/invoice.md |
| "**scan my Gmail** for receipts / invoices and record them" | read candidates via Gmail MCP → extract to Entry JSON with `source_id` = Gmail message id → `easybooks gmail record --json '<json>' --dry-run` → confirm → rerun | references/gmail.md |
| "**find / search** a transaction by type / category / date / amount / text" | `easybooks tx list [--type income\|expense] [--classification business\|mixed\|personal\|unclassified] [--review needs_review\|reviewed] [--from YYYY-MM-DD] [--to YYYY-MM-DD] [--query <q>] [--limit n]` | references/record.md |
| "**confirm** a transaction (clear needs-review without changing classification)" | `easybooks tx confirm <id>` | references/record.md |
| "**edit** a transaction (amount / date / description / category / notes)" | `easybooks tx update <id> [--amount <d>] [--date YYYY-MM-DD] [--description "<t>"] [--category <name>] [--notes "<t>"] [--dry-run]` | references/record.md |
| "view receipt **URL** for a transaction" | `easybooks tx receipt-url <id>` (returns signed URL) | references/record.md |
| "**dashboard** summary (income / expenses / net / outstanding / tax estimate)" | `easybooks dashboard [--year <YYYY>]` | references/record.md |
| "list my **categories**" | `easybooks categories list [--type income\|expense]` | references/record.md |
| "**manage categories** (create / view)" | `easybooks categories create --name <n> --type income\|expense [--tax-deductible]` | references/record.md |
| "**manage clients** (list / create / update / delete)" | `easybooks clients list`, `easybooks clients create --name <n> [--email --phone --address --notes]`, `easybooks clients update <id> ...`, `easybooks clients delete <id>` | references/invoice.md |
| "get **invoice details**" | `easybooks invoice get <id>` | references/invoice.md |
| "**mark invoice** paid / unpaid" | `easybooks invoice mark <id> --status paid\|unpaid` | references/invoice.md |
| "**download invoice** PDF" | `easybooks invoice pdf <id> [--out <path>]` | references/invoice.md |
| "**invoice stats** (counts / amounts by status)" | `easybooks invoice stats [--year <YYYY>]` | references/invoice.md |
| "list my **invoices** [that are unpaid/draft]" | `easybooks invoices list [--status <s>]` | references/invoice.md |
| "is EasyBooks **healthy** / which backend am I on / token still valid" | `easybooks --json doctor` (local config + backend round-trip + version) | SKILL.md |
| "**connect** EasyBooks / save my platform token / set it up" | `references/connect.md` → `easybooks login --token-stdin [--base-url <url>]` | references/connect.md |
| "EasyBooks **out of date**?" | `easybooks --json doctor --no-fetch --check-upgrade` | references/connect.md |

## Skill router by user intent

| User says (any phrasing) | Skill | Entry point |
|---|---|---|
| "connect / set up EasyBooks / save my key" | [connect](connect.md) | `easybooks login` then `whoami` / `doctor` |
| "log an expense / income", "record this receipt / file / image / PDF / statement", "fix a classification", "attach a receipt" | [record](record.md) | `expense add` / `income add` / `tx import-json` / `tx reclassify` / `tx attach-receipt` |
| "create an invoice", "send invoice X", "list my clients / invoices" | [invoice](invoice.md) | `invoice create` / `invoice send` / `clients` / `invoices list` |
| "scan my Gmail for receipts / invoices and record them" | [gmail](gmail.md) | Gmail MCP read → `gmail record` |
| "is my plugin healthy / which backend am I on" | SKILL.md | `easybooks --json doctor` |

## Complete CLI surface by responsibility

| Responsibility | Commands |
|---|---|
| Connect / health | `login`, `whoami`, `doctor` |
| List / search transactions | `tx list`, `tx receipt-url <id>`, `tx confirm <id>` |
| Record transactions | `income add ...`, `expense add ...`, `tx import-json --json '<json>' [--dry-run]` |
| Edit / classify transactions | `tx update <id> ...`, `tx reclassify <id> --class business\|mixed\|personal [--learn]`, `tx attach-receipt <id> --file <path>` |
| Dashboard / summaries | `dashboard [--year YYYY]` |
| Categories | `categories list [--type income\|expense]`, `categories create --name <n> --type income\|expense [--tax-deductible]` |
| Clients | `clients list`, `clients create ...`, `clients update <id> ...`, `clients delete <id>` |
| Invoices | `invoice get <id>`, `invoice create --json '<json>' [--dry-run]`, `invoice send <invoice_id>`, `invoice mark <id> --status paid\|unpaid`, `invoice pdf <id> [--out <path>]`, `invoice stats [--year YYYY]`, `invoices list [--status <s>]` |
| Rules (auto-categorization) | `rules list`, `rules show <id>`, `rules create --json '<json>'`, `rules delete <id>`, `rules enable\|disable <id>`, `rules apply --scope <all\|unclassified\|selected> [--ids ..] [--rule-ids ..] [--only-auto-apply] [--commit]` |
| Gmail (v1) | `gmail record --json '<json>' [--dry-run]`, `gmail sync` |
| Public command list | `commands` |

Treat `"$EASYBOOKS_BIN" --help` as runtime truth when docs and code drift.

## Rules — auto-categorization command map

| User intent | Command |
|---|---|
| "list my rules" | `easybooks rules list` |
| "show rule `<id>`" | `easybooks rules show <rule_id>` |
| "create a rule" | `easybooks rules create --json '<rule json>'` |
| "delete rule `<id>`" | `easybooks rules delete <rule_id>` |
| "enable / turn on rule `<id>`" | `easybooks rules enable <rule_id>` |
| "disable / pause rule `<id>`" | `easybooks rules disable <rule_id>` |
| "apply / run my rules over transactions" | `easybooks rules apply --scope <all\|unclassified\|selected> [--ids a,b] [--rule-ids r1,r2] [--only-auto-apply] [--commit]` |

Rule shape, preview/commit behavior, and examples remain in SKILL §R.
