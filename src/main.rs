//! Deterministic generator for synthetic banking, non-life insurance and life
//! underwriting datasets.
//!
//! Nothing here is real: every name, date and amount comes from a seeded
//! pseudo random number generator, so the same seed and counts always produce
//! byte-identical output.
//!
//! The datasets are written separately, into separate files, because a bank and
//! an insurer do not share a ledger and a carrier's regulatory reporting is split
//! by line of business. Each dataset is emitted as SQL (for a Postgres load) and
//! as one CSV per table (for a spreadsheet).
//!
//!   cargo run -- --dataset all --seed 20260101
//!   cargo run -- --dataset retail_banking --customers 1000 --transactions 50000
//!   cargo run -- --dataset life --life-policies 4000
//!
//! The three datasets are `retail_banking`, `non_life` and `life`. The first is a
//! retail bank, the second a general (non-life) carrier, the third a life carrier's
//! underwriting inputs. `banking`, `insurance` and `both` are accepted as shorthand.
//!
//! The life dataset carries underwriting inputs only: policy issue data plus the
//! assumption tables a model consumes. It contains no calculated projections, on
//! purpose: a projection with no model to validate it is noise.
//!
//! Only the standard library is used.

use std::collections::{HashMap, HashSet};
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};

#[derive(Debug)]
struct Cli {
    out: PathBuf,
    seed: u64,
    dataset: String,
    customers: i64,
    transactions: i64,
    loans: i64,
    policyholders: i64,
    policies: i64,
    claims: i64,
    life_policyholders: i64,
    life_policies: i64,
}

/// Canonical dataset label. The shorthand names are kept so an older command or
/// script still works; everything downstream uses the canonical label.
fn canonical_dataset(name: &str) -> Option<&'static str> {
    match name {
        "banking" | "retail_banking" | "retail-banking" => Some("retail_banking"),
        "insurance" | "non_life" | "non-life" | "general_insurance" => Some("non_life"),
        "life" | "life_insurance" | "life-insurance" => Some("life"),
        "both" => Some("both"),
        "all" => Some("all"),
        _ => None,
    }
}

fn parse_cli() -> Cli {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let mut cli = Cli {
        out: PathBuf::from("out"),
        seed: 20260101,
        dataset: "both".to_string(),
        customers: 5000,
        transactions: 200_000,
        loans: 1200,
        policyholders: 4000,
        policies: 8000,
        claims: 25_000,
        life_policyholders: 3000,
        life_policies: 6000,
    };
    let mut i = 0;
    while i < args.len() {
        let flag = args[i].clone();
        if flag == "--help" {
            println!(
                "usage: synthetic-financial-data [--out DIR] [--dataset retail_banking|non_life|life|both|all] \
                 [--seed N] [--customers N] [--transactions N] [--loans N] [--policyholders N] \
                 [--policies N] [--claims N] [--life-policyholders N] [--life-policies N]"
            );
            std::process::exit(0);
        }
        if i + 1 >= args.len() {
            eprintln!("error: {flag} needs a value");
            std::process::exit(1);
        }
        let value = args[i + 1].clone();
        let set = |field: &mut i64, v: &str| -> Result<(), String> {
            v.parse::<i64>()
                .map_err(|_| format!("{flag} must be a whole number, got '{v}'"))
                .map(|n| *field = n)
        };
        match flag.as_str() {
            "--out" => cli.out = PathBuf::from(value),
            "--seed" => {
                cli.seed = value
                    .parse::<u64>()
                    .map_err(|e| format!("bad seed: {e}"))
                    .unwrap_or(cli.seed)
            }
            "--dataset" => {
                let Some(canonical) = canonical_dataset(&value.to_lowercase()) else {
                    eprintln!(
                        "error: --dataset must be retail_banking, non_life, life, both or all"
                    );
                    std::process::exit(1);
                };
                cli.dataset = canonical.to_string();
            }
            "--customers" => set(&mut cli.customers, &value).unwrap_or_else(|e| {
                eprintln!("error: {e}");
                std::process::exit(1);
            }),
            "--transactions" => set(&mut cli.transactions, &value).unwrap_or_else(|e| {
                eprintln!("error: {e}");
                std::process::exit(1);
            }),
            "--loans" => set(&mut cli.loans, &value).unwrap_or_else(|e| {
                eprintln!("error: {e}");
                std::process::exit(1);
            }),
            "--policyholders" => set(&mut cli.policyholders, &value).unwrap_or_else(|e| {
                eprintln!("error: {e}");
                std::process::exit(1);
            }),
            "--policies" => set(&mut cli.policies, &value).unwrap_or_else(|e| {
                eprintln!("error: {e}");
                std::process::exit(1);
            }),
            "--claims" => set(&mut cli.claims, &value).unwrap_or_else(|e| {
                eprintln!("error: {e}");
                std::process::exit(1);
            }),
            "--life-policyholders" => {
                set(&mut cli.life_policyholders, &value).unwrap_or_else(|e| {
                    eprintln!("error: {e}");
                    std::process::exit(1);
                })
            }
            "--life-policies" => set(&mut cli.life_policies, &value).unwrap_or_else(|e| {
                eprintln!("error: {e}");
                std::process::exit(1);
            }),
            other => {
                eprintln!("error: unknown flag {other}");
                std::process::exit(1);
            }
        }
        i += 2;
    }
    if cli.customers < 1
        || cli.transactions < 1
        || cli.policyholders < 1
        || cli.policies < 1
        || cli.life_policyholders < 1
        || cli.life_policies < 1
    {
        eprintln!("error: row counts must be at least 1");
        std::process::exit(1);
    }
    cli
}

/// splitmix64. Deterministic, dependency free, good enough for synthetic data.
struct Rng {
    state: u64,
}

impl Rng {
    fn new(seed: u64) -> Rng {
        Rng { state: seed }
    }

    fn next(&mut self) -> u64 {
        self.state = self.state.wrapping_add(0x9e3779b97f4a7c15);
        let mut z = self.state;
        z = (z ^ (z >> 30)).wrapping_mul(0xbf58476d1ce4e5b9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94d049bb133111eb);
        z ^ (z >> 31)
    }

    fn pick<T: Copy>(&mut self, options: &[T]) -> T {
        options[(self.next() % options.len() as u64) as usize]
    }

    fn chance(&mut self, percent: u64) -> bool {
        (self.next() % 100) < percent
    }

    fn range(&mut self, low: i64, high: i64) -> i64 {
        low + (self.next() % (high - low + 1) as u64) as i64
    }
}

fn date_from_days(days: i64) -> String {
    if days < 0 {
        return "1970-01-01".to_string();
    }
    let leap = |y: i64| (y % 4 == 0 && y % 100 != 0) || y % 400 == 0;
    let mut year = 1970;
    let mut remaining = days;
    while remaining >= 365 + leap(year) as i64 {
        remaining -= 365 + leap(year) as i64;
        year += 1;
    }
    let month_lengths = [31, 28, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31];
    let mut month = 0;
    loop {
        let length = month_lengths[month] + (month == 1 && leap(year)) as i64;
        if remaining < length {
            break;
        }
        remaining -= length;
        month += 1;
    }
    format!("{year:04}-{:02}-{:02}", month + 1, remaining + 1)
}

/// Money is held as integer cents and printed with two decimals: no float
/// rounding ever reaches the output files.
fn money(cents: i64) -> String {
    let sign = if cents < 0 { "-" } else { "" };
    let abs = cents.abs();
    format!("{sign}{}.{:02}", abs / 100, abs % 100)
}

/// One column: name, Postgres type, nullability. Types are written out per
/// column instead of guessed from the name, so a text reference such as
/// `ID-000001` never ends up declared as a number.
#[derive(Clone)]
struct Column {
    name: &'static str,
    ty: &'static str,
    not_null: bool,
}

fn column(name: &'static str, ty: &'static str) -> Column {
    Column {
        name,
        ty,
        not_null: true,
    }
}

fn nullable_column(name: &'static str, ty: &'static str) -> Column {
    Column {
        name,
        ty,
        not_null: false,
    }
}

/// A child row must reference an existing row in the parent table. The same
/// declaration is used for the schema and for the integrity tests.
#[derive(Clone)]
struct ForeignKey {
    column: &'static str,
    parent_table: &'static str,
    parent_column: &'static str,
}

fn foreign_key(
    column: &'static str,
    parent_table: &'static str,
    parent_column: &'static str,
) -> ForeignKey {
    ForeignKey {
        column,
        parent_table,
        parent_column,
    }
}

/// One table, its columns and its rows. SQL and CSV are both derived from this,
/// so the two representations cannot drift apart.
#[derive(Clone)]
struct Table {
    name: &'static str,
    primary_key: &'static str,
    columns: Vec<Column>,
    foreign_keys: Vec<ForeignKey>,
    rows: Vec<Vec<String>>,
}

impl Table {
    fn names(&self) -> String {
        self.columns
            .iter()
            .map(|c| c.name)
            .collect::<Vec<_>>()
            .join(", ")
    }

    fn column_index(&self, name: &str) -> Option<usize> {
        self.columns.iter().position(|c| c.name == name)
    }

    fn sql(&self) -> String {
        let mut out: Vec<u8> = Vec::new();
        for chunk in self.rows.chunks(500) {
            writeln!(out, "INSERT INTO {} ({}) VALUES", self.name, self.names()).unwrap();
            for (i, row) in chunk.iter().enumerate() {
                let values: Vec<String> = row.iter().map(|v| quote_sql(v)).collect();
                let sep = if i + 1 == chunk.len() { ";\n" } else { ",\n" };
                writeln!(out, "({}){}", values.join(", "), sep).unwrap();
            }
        }
        String::from_utf8(out).unwrap_or_default()
    }

    fn csv(&self) -> String {
        let mut out = String::new();
        out.push_str(
            &self
                .columns
                .iter()
                .map(|c| c.name)
                .collect::<Vec<_>>()
                .join(","),
        );
        out.push('\n');
        for row in &self.rows {
            let cells: Vec<String> = row.iter().map(|v| quote_csv(v)).collect();
            out.push_str(&cells.join(","));
            out.push('\n');
        }
        out
    }

    fn declared_columns(&self) -> String {
        self.columns
            .iter()
            .map(|c| c.name)
            .collect::<Vec<_>>()
            .join(", ")
    }
}

fn quote_sql(value: &str) -> String {
    if value == "NULL" {
        return "NULL".to_string();
    }
    if value.parse::<f64>().is_ok() {
        return value.to_string();
    }
    format!("'{}'", value.replace('\'', "''"))
}

fn quote_csv(value: &str) -> String {
    if value.contains(',') || value.contains('"') || value.contains('\n') {
        return format!("\"{}\"", value.replace('"', "\"\""));
    }
    value.to_string()
}

const REGIONS: [&str; 8] = [
    "Central",
    "Coastal",
    "Eastern",
    "Highlands",
    "Northern",
    "Western",
    "Southern",
    "Metro",
];
const BANK_PRODUCTS: [&str; 5] = ["current", "savings", "fixed deposit", "overdraft", "loan"];
const CHANNELS: [&str; 6] = ["counter", "ATM", "mobile", "online", "agent", "card"];
const TXN_CATEGORIES: [&str; 8] = [
    "salary",
    "utilities",
    "retail",
    "transfer",
    "fees",
    "loan repayment",
    "insurance premium",
    "supply",
];
const LOAN_TYPES: [&str; 5] = ["personal", "mortgage", "business", "auto", "credit card"];
const LOAN_STATUSES: [&str; 3] = ["active", "settled", "default"];
const KYC_OUTCOMES: [&str; 4] = ["cleared", "cleared", "review", "escalated"];
const RISK_BANDS: [&str; 3] = ["low", "medium", "high"];

// Solvency II Annex I lines of business, abbreviated to the common non-life
// lines a general carrier writes. Life business has a different technical
// basis and is deliberately out of scope here.
const LINES_OF_BUSINESS: [&str; 6] = [
    "motor",
    "property",
    "liability",
    "medical expense",
    "income protection",
    "marine and cargo",
];
const COVERAGES: [&str; 6] = [
    "collision",
    "theft",
    "fire and flood",
    "third party liability",
    "hospitalisation",
    "loss of earnings",
];
const EXCLUSIONS: [&str; 6] = [
    "wear and tear",
    "unauthorised modification",
    "pre-existing condition",
    "late notification",
    "acts of nature",
    "commercial use of a personal asset",
];
const CLAIM_STATUSES: [&str; 4] = ["open", "approved", "partially paid", "rejected"];
const ADJUSTMENT_REASONS: [&str; 5] = [
    "documentation",
    "reassessment",
    "deductible applied",
    "duplicate removal",
    "subrogation recovery",
];
const ENDORSEMENT_TYPES: [&str; 4] = [
    "add coverage",
    "remove coverage",
    "change sum insured",
    "address change",
];

fn retail_banking_tables(cli: &Cli, rng: &mut Rng, base_day: i64) -> Vec<Table> {
    let branches = 40;
    let mut tables = Vec::new();

    let mut rows = Vec::new();
    for id in 1..=branches {
        rows.push(vec![
            id.to_string(),
            format!("BRANCH-{id:04}"),
            format!("Branch {id}"),
            rng.pick(&REGIONS).to_string(),
            date_from_days(base_day - rng.range(0, 1500)),
        ]);
    }
    tables.push(Table {
        name: "branches",
        primary_key: "branch_id",
        columns: vec![
            column("branch_id", "BIGINT"),
            column("branch_code", "TEXT"),
            column("name", "TEXT"),
            column("region", "TEXT"),
            column("opened_date", "DATE"),
        ],
        foreign_keys: vec![],
        rows,
    });

    let mut rows = Vec::new();
    for id in 1..=cli.customers {
        rows.push(vec![
            id.to_string(),
            format!("CUST-{id:06}"),
            format!("Customer {id}"),
            date_from_days(base_day - rng.range(6000, 12000)),
            rng.pick(&REGIONS).to_string(),
            rng.pick(&["retail", "sme", "corporate"]).to_string(),
            "individual".to_string(),
            format!("PHONE-{id:06}"),
            format!("ID-{id:06}"),
            date_from_days(base_day - rng.range(0, 2000)),
        ]);
    }
    tables.push(Table {
        name: "customers",
        primary_key: "customer_id",
        columns: vec![
            column("customer_id", "BIGINT"),
            column("customer_reference", "TEXT"),
            column("full_name", "TEXT"),
            column("date_of_birth", "DATE"),
            column("region", "TEXT"),
            column("segment", "TEXT"),
            column("customer_type", "TEXT"),
            column("phone", "TEXT"),
            column("national_id", "TEXT"),
            column("opened_date", "DATE"),
        ],
        foreign_keys: vec![],
        rows,
    });

    let mut account_id = 0;
    let mut opening_cents: Vec<i64> = Vec::new();
    let mut rows = Vec::new();
    for customer in 1..=cli.customers {
        for _ in 0..rng.range(1, 4) {
            account_id += 1;
            let opening = rng.range(0, 50_000_000);
            opening_cents.push(opening);
            rows.push(vec![
                account_id.to_string(),
                format!("ACC-{account_id:09}"),
                customer.to_string(),
                rng.range(1, branches).to_string(),
                rng.pick(&BANK_PRODUCTS).to_string(),
                money(opening),
                money(opening),
                date_from_days(base_day - rng.range(0, 1800)),
                rng.pick(&["active", "active", "active", "dormant", "closed"])
                    .to_string(),
            ]);
        }
    }

    // The ledger is generated after the accounts so each account's balance is its
    // opening balance plus the posted entries against it. Pending and reversed
    // entries do not move the balance, which makes
    // "balance = opening_balance + sum of posted entries" a check a query can run.
    // A debit is capped at the balance available at the point it is generated, so
    // no account ends up overdrawn.
    let mut net_cents = vec![0i64; account_id as usize + 1];
    let mut ledger = Vec::new();
    for txn in 1..=cli.transactions {
        let account = rng.range(1, account_id) as usize;
        let mut amount = rng.range(100, 2_000_000);
        let status = rng
            .pick(&["posted", "posted", "posted", "pending", "reversed"])
            .to_string();
        let mut direction = if rng.chance(60) { "credit" } else { "debit" };
        if direction == "debit" {
            let available = opening_cents[account - 1] + net_cents[account];
            if available <= 0 {
                direction = "credit";
            } else if amount > available {
                amount = available;
            }
        }
        if status == "posted" {
            net_cents[account] += if direction == "credit" {
                amount
            } else {
                -amount
            };
        }
        ledger.push(vec![
            txn.to_string(),
            format!("TXN-{txn:010}"),
            account.to_string(),
            date_from_days(base_day + rng.range(0, 730)),
            money(amount),
            direction.to_string(),
            rng.pick(&CHANNELS).to_string(),
            rng.pick(&TXN_CATEGORIES).to_string(),
            status,
        ]);
    }
    let mut accounts = Table {
        name: "accounts",
        primary_key: "account_id",
        columns: vec![
            column("account_id", "BIGINT"),
            column("account_number", "TEXT"),
            column("customer_id", "BIGINT"),
            column("branch_id", "BIGINT"),
            column("product", "TEXT"),
            column("opening_balance", "NUMERIC(12,2)"),
            column("balance", "NUMERIC(12,2)"),
            column("opened_date", "DATE"),
            column("status", "TEXT"),
        ],
        foreign_keys: vec![
            foreign_key("customer_id", "customers", "customer_id"),
            foreign_key("branch_id", "branches", "branch_id"),
        ],
        rows,
    };
    let balance_index = accounts
        .column_index("balance")
        .expect("accounts has a balance column");
    for (index, row) in accounts.rows.iter_mut().enumerate() {
        row[balance_index] = money(opening_cents[index] + net_cents[index + 1]);
    }
    tables.push(accounts);

    tables.push(Table {
        name: "transactions",
        primary_key: "transaction_id",
        columns: vec![
            column("transaction_id", "BIGINT"),
            column("transaction_reference", "TEXT"),
            column("account_id", "BIGINT"),
            column("transaction_date", "DATE"),
            column("amount", "NUMERIC(12,2)"),
            column("direction", "TEXT"),
            column("channel", "TEXT"),
            column("category", "TEXT"),
            column("status", "TEXT"),
        ],
        foreign_keys: vec![foreign_key("account_id", "accounts", "account_id")],
        rows: ledger,
    });

    let mut rows = Vec::new();
    for id in 1..=cli.loans {
        let principal = rng.range(100_000, 20_000_000);
        let term = rng.range(12, 240);
        let paid = rng.range(0, principal);
        let start_day = base_day - rng.range(0, 900);
        rows.push(vec![
            id.to_string(),
            format!("LOAN-{id:06}"),
            rng.range(1, cli.customers).to_string(),
            rng.pick(&LOAN_TYPES).to_string(),
            money(principal),
            money(rng.range(300, 2000)),
            term.to_string(),
            date_from_days(start_day),
            date_from_days(start_day + term * 30),
            money(principal - paid),
            rng.pick(&LOAN_STATUSES).to_string(),
        ]);
    }
    tables.push(Table {
        name: "loans",
        primary_key: "loan_id",
        columns: vec![
            column("loan_id", "BIGINT"),
            column("loan_reference", "TEXT"),
            column("customer_id", "BIGINT"),
            column("loan_type", "TEXT"),
            column("principal", "NUMERIC(12,2)"),
            column("interest_rate", "NUMERIC(5,2)"),
            column("term_months", "BIGINT"),
            column("start_date", "DATE"),
            column("maturity_date", "DATE"),
            column("outstanding_balance", "NUMERIC(12,2)"),
            column("status", "TEXT"),
        ],
        foreign_keys: vec![foreign_key("customer_id", "customers", "customer_id")],
        rows,
    });

    let mut review_id = 0;
    let mut rows = Vec::new();
    for customer in 1..=cli.customers {
        for _ in 0..rng.range(1, 3) {
            review_id += 1;
            rows.push(vec![
                review_id.to_string(),
                customer.to_string(),
                date_from_days(base_day + rng.range(0, 730)),
                rng.pick(&KYC_OUTCOMES).to_string(),
                rng.pick(&RISK_BANDS).to_string(),
                format!("REVIEWER-{}", rng.range(1, 60)),
            ]);
        }
    }
    tables.push(Table {
        name: "kyc_reviews",
        primary_key: "review_id",
        columns: vec![
            column("review_id", "BIGINT"),
            column("customer_id", "BIGINT"),
            column("reviewed_date", "DATE"),
            column("outcome", "TEXT"),
            column("risk_band", "TEXT"),
            column("reviewer_code", "TEXT"),
        ],
        foreign_keys: vec![foreign_key("customer_id", "customers", "customer_id")],
        rows,
    });

    tables
}

fn non_life_tables(cli: &Cli, rng: &mut Rng, base_day: i64) -> Vec<Table> {
    let branches = 25;
    let mut tables = Vec::new();

    let mut rows = Vec::new();
    for id in 1..=branches {
        rows.push(vec![
            id.to_string(),
            format!("OFFICE-{id:03}"),
            format!("Office {id}"),
            rng.pick(&REGIONS).to_string(),
        ]);
    }
    tables.push(Table {
        name: "offices",
        primary_key: "office_id",
        columns: vec![
            column("office_id", "BIGINT"),
            column("office_code", "TEXT"),
            column("name", "TEXT"),
            column("region", "TEXT"),
        ],
        foreign_keys: vec![],
        rows,
    });

    let mut rows = Vec::new();
    for id in 1..=cli.policyholders {
        rows.push(vec![
            id.to_string(),
            format!("PH-{id:06}"),
            format!("Policyholder {id}"),
            rng.pick(&["individual", "individual", "company"])
                .to_string(),
            rng.pick(&REGIONS).to_string(),
            format!("AGENT-{}", rng.range(1, 200)),
        ]);
    }
    tables.push(Table {
        name: "policyholders",
        primary_key: "policyholder_id",
        columns: vec![
            column("policyholder_id", "BIGINT"),
            column("policyholder_reference", "TEXT"),
            column("name", "TEXT"),
            column("policyholder_type", "TEXT"),
            column("region", "TEXT"),
            column("agent_code", "TEXT"),
        ],
        foreign_keys: vec![],
        rows,
    });

    let mut rows = Vec::new();
    for id in 1..=cli.policies {
        let premium = rng.range(50_000, 2_000_000);
        let start_day = base_day - rng.range(0, 700);
        rows.push(vec![
            id.to_string(),
            format!("POL-{id:07}"),
            rng.range(1, cli.policyholders).to_string(),
            rng.range(1, branches).to_string(),
            rng.pick(&LINES_OF_BUSINESS).to_string(),
            money(premium),
            money(rng.range(500_000, 50_000_000)),
            money(rng.range(5_000, 200_000)),
            rng.pick(&["monthly", "annual", "quarterly"]).to_string(),
            date_from_days(start_day),
            date_from_days(start_day + 365),
            rng.pick(&["in force", "in force", "in force", "expired", "cancelled"])
                .to_string(),
        ]);
    }
    tables.push(Table {
        name: "policies",
        primary_key: "policy_id",
        columns: vec![
            column("policy_id", "BIGINT"),
            column("policy_number", "TEXT"),
            column("policyholder_id", "BIGINT"),
            column("office_id", "BIGINT"),
            column("line_of_business", "TEXT"),
            column("premium", "NUMERIC(12,2)"),
            column("sum_insured", "NUMERIC(12,2)"),
            column("deductible", "NUMERIC(12,2)"),
            column("payment_frequency", "TEXT"),
            column("inception_date", "DATE"),
            column("expiry_date", "DATE"),
            column("status", "TEXT"),
        ],
        foreign_keys: vec![
            foreign_key("policyholder_id", "policyholders", "policyholder_id"),
            foreign_key("office_id", "offices", "office_id"),
        ],
        rows,
    });

    let mut coverage_id = 0;
    let mut rows = Vec::new();
    for policy in 1..=cli.policies {
        for _ in 0..rng.range(1, 4) {
            coverage_id += 1;
            rows.push(vec![
                coverage_id.to_string(),
                policy.to_string(),
                rng.pick(&COVERAGES).to_string(),
                money(rng.range(100_000, 20_000_000)),
                money(rng.range(1_000, 100_000)),
            ]);
        }
    }
    tables.push(Table {
        name: "coverages",
        primary_key: "coverage_id",
        columns: vec![
            column("coverage_id", "BIGINT"),
            column("policy_id", "BIGINT"),
            column("coverage_type", "TEXT"),
            column("coverage_limit", "NUMERIC(12,2)"),
            column("deductible", "NUMERIC(12,2)"),
        ],
        foreign_keys: vec![foreign_key("policy_id", "policies", "policy_id")],
        rows,
    });

    let mut exclusion_id = 0;
    let mut rows = Vec::new();
    for policy in 1..=cli.policies {
        for _ in 0..rng.range(1, 4) {
            exclusion_id += 1;
            rows.push(vec![
                exclusion_id.to_string(),
                policy.to_string(),
                format!("EX-{exclusion_id:05}"),
                rng.pick(&EXCLUSIONS).to_string(),
            ]);
        }
    }
    tables.push(Table {
        name: "exclusions",
        primary_key: "exclusion_id",
        columns: vec![
            column("exclusion_id", "BIGINT"),
            column("policy_id", "BIGINT"),
            column("code", "TEXT"),
            column("description", "TEXT"),
        ],
        foreign_keys: vec![foreign_key("policy_id", "policies", "policy_id")],
        rows,
    });

    let mut rows = Vec::new();
    for id in 1..=cli.claims {
        let incident_day = base_day - rng.range(0, 500);
        let report_day = incident_day + rng.range(0, 30);
        let status = rng.pick(&CLAIM_STATUSES);
        let claimed = rng.range(5_000, 5_000_000);
        let paid = match status {
            "approved" | "partially paid" => rng.range(1_000, claimed),
            _ => 0,
        };
        let reserve = match status {
            "open" => rng.range(1_000, claimed),
            _ => 0,
        };
        rows.push(vec![
            id.to_string(),
            format!("CLAIM-{id:07}"),
            rng.range(1, cli.policies).to_string(),
            date_from_days(incident_day),
            date_from_days(report_day),
            if rng.chance(80) {
                date_from_days(report_day + rng.range(1, 90))
            } else {
                "NULL".to_string()
            },
            status.to_string(),
            money(claimed),
            money(paid),
            money(reserve),
        ]);
    }
    tables.push(Table {
        name: "claims",
        primary_key: "claim_id",
        columns: vec![
            column("claim_id", "BIGINT"),
            column("claim_reference", "TEXT"),
            column("policy_id", "BIGINT"),
            column("incident_date", "DATE"),
            column("report_date", "DATE"),
            nullable_column("decision_date", "DATE"),
            column("status", "TEXT"),
            column("claimed_amount", "NUMERIC(12,2)"),
            column("paid_amount", "NUMERIC(12,2)"),
            column("reserve", "NUMERIC(12,2)"),
        ],
        foreign_keys: vec![foreign_key("policy_id", "policies", "policy_id")],
        rows,
    });

    let mut adjustment_id = 0;
    let mut rows = Vec::new();
    for claim in 1..=cli.claims {
        if !rng.chance(33) {
            continue;
        }
        adjustment_id += 1;
        rows.push(vec![
            adjustment_id.to_string(),
            claim.to_string(),
            date_from_days(base_day + rng.range(0, 600)),
            money(rng.range(-500_000, 500_000)),
            rng.pick(&ADJUSTMENT_REASONS).to_string(),
        ]);
    }
    tables.push(Table {
        name: "claim_adjustments",
        primary_key: "adjustment_id",
        columns: vec![
            column("adjustment_id", "BIGINT"),
            column("claim_id", "BIGINT"),
            column("adjustment_date", "DATE"),
            column("amount", "NUMERIC(12,2)"),
            column("reason", "TEXT"),
        ],
        foreign_keys: vec![foreign_key("claim_id", "claims", "claim_id")],
        rows,
    });

    let mut endorsement_id = 0;
    let mut rows = Vec::new();
    for policy in 1..=cli.policies {
        if !rng.chance(40) {
            continue;
        }
        endorsement_id += 1;
        rows.push(vec![
            endorsement_id.to_string(),
            policy.to_string(),
            date_from_days(base_day + rng.range(0, 300)),
            rng.pick(&ENDORSEMENT_TYPES).to_string(),
            money(rng.range(-100_000, 300_000)),
        ]);
    }
    tables.push(Table {
        name: "endorsements",
        primary_key: "endorsement_id",
        columns: vec![
            column("endorsement_id", "BIGINT"),
            column("policy_id", "BIGINT"),
            column("effective_date", "DATE"),
            column("endorsement_type", "TEXT"),
            column("premium_change", "NUMERIC(12,2)"),
        ],
        foreign_keys: vec![foreign_key("policy_id", "policies", "policy_id")],
        rows,
    });

    tables
}

const LIFE_PRODUCTS: [&str; 4] = ["term life", "whole life", "endowment", "critical illness"];
const OCCUPATION_CLASSES: [&str; 6] = [
    "clerical",
    "professional",
    "skilled trade",
    "manual",
    "self-employed",
    "retired",
];

/// Rates are written as exact decimal strings from integer micro-units, so no
/// float ever reaches the output files. The table shapes follow what a UK life
/// carrier feeds a model: mortality by gender, smoking status and age; morbidity
/// by product and age; lapse by product and duration; expense by product and
/// months in force; a discount curve by month. The numbers are invented here, not
/// lifted from a published mortality table.
fn micro_rate(micros: i64) -> String {
    let sign = if micros < 0 { "-" } else { "" };
    let abs = micros.abs();
    format!("{sign}{}.{:06}", abs / 1_000_000, abs % 1_000_000)
}

fn monthly_mortality_micros(age: i64, gender: &str, smoking: &str) -> i64 {
    let base = if age < 40 {
        20 + (age - 18) * 4
    } else if age < 60 {
        100 + (age - 40) * 20
    } else if age < 80 {
        500 + (age - 60) * 60
    } else {
        1700 + (age - 80) * 150
    };
    let base = if gender == "F" { base * 3 / 4 } else { base };
    if smoking == "smoker" { base * 2 } else { base }
}

fn life_tables(cli: &Cli, rng: &mut Rng, base_day: i64) -> Vec<Table> {
    let offices = 20;
    let mut tables = Vec::new();

    let mut rows = Vec::new();
    for id in 1..=offices {
        rows.push(vec![
            id.to_string(),
            format!("LIFE-OFFICE-{id:03}"),
            format!("Office {id}"),
            rng.pick(&REGIONS).to_string(),
        ]);
    }
    tables.push(Table {
        name: "offices",
        primary_key: "office_id",
        columns: vec![
            column("office_id", "BIGINT"),
            column("office_code", "TEXT"),
            column("name", "TEXT"),
            column("region", "TEXT"),
        ],
        foreign_keys: vec![],
        rows,
    });

    let mut rows = Vec::new();
    for id in 1..=cli.life_policyholders {
        rows.push(vec![
            id.to_string(),
            format!("LPH-{id:06}"),
            format!("Policyholder {id}"),
            rng.pick(&["individual", "individual", "company"])
                .to_string(),
            rng.pick(&REGIONS).to_string(),
            format!("AGENT-{}", rng.range(1, 200)),
        ]);
    }
    tables.push(Table {
        name: "policyholders",
        primary_key: "policyholder_id",
        columns: vec![
            column("policyholder_id", "BIGINT"),
            column("policyholder_reference", "TEXT"),
            column("name", "TEXT"),
            column("policyholder_type", "TEXT"),
            column("region", "TEXT"),
            column("agent_code", "TEXT"),
        ],
        foreign_keys: vec![],
        rows,
    });

    let mut rows = Vec::new();
    for id in 1..=cli.life_policies {
        let product = rng.pick(&LIFE_PRODUCTS);
        let gender = rng.pick(&["F", "M"]);
        let smoking = rng.pick(&["non-smoker", "non-smoker", "smoker"]);
        let age = rng.range(18, 70);
        let sum_assured = rng.range(1_000_000, 100_000_000);
        let premium = sum_assured * (10 + age) / 1000;
        let guaranteed = match product {
            "endowment" => sum_assured / 2,
            "critical illness" => 0,
            _ => sum_assured,
        };
        let term = match product {
            "whole life" => "NULL".to_string(),
            "endowment" => rng.range(10, 25).to_string(),
            _ => rng.range(5, 35).to_string(),
        };
        rows.push(vec![
            id.to_string(),
            format!("LIFE-POL-{id:07}"),
            rng.range(1, cli.life_policyholders).to_string(),
            rng.range(1, offices).to_string(),
            product.to_string(),
            gender.to_string(),
            age.to_string(),
            smoking.to_string(),
            rng.pick(&OCCUPATION_CLASSES).to_string(),
            date_from_days(base_day - rng.range(0, 1500)),
            term,
            money(sum_assured),
            money(premium),
            rng.pick(&["monthly", "annual", "quarterly"]).to_string(),
            rng.pick(&["in force", "in force", "in force", "lapsed", "matured"])
                .to_string(),
            money(guaranteed),
        ]);
    }
    tables.push(Table {
        name: "policies",
        primary_key: "policy_id",
        columns: vec![
            column("policy_id", "BIGINT"),
            column("policy_number", "TEXT"),
            column("policyholder_id", "BIGINT"),
            column("office_id", "BIGINT"),
            column("product", "TEXT"),
            column("gender", "TEXT"),
            column("age_at_issue", "BIGINT"),
            column("smoking_status", "TEXT"),
            column("occupation_class", "TEXT"),
            column("issue_date", "DATE"),
            nullable_column("term_years", "BIGINT"),
            column("sum_assured", "NUMERIC(12,2)"),
            column("annual_premium", "NUMERIC(12,2)"),
            column("premium_frequency", "TEXT"),
            column("status", "TEXT"),
            column("guaranteed_sum_assured", "NUMERIC(12,2)"),
        ],
        foreign_keys: vec![
            foreign_key("policyholder_id", "policyholders", "policyholder_id"),
            foreign_key("office_id", "offices", "office_id"),
        ],
        rows,
    });

    let mut rows = Vec::new();
    let mut assumption_id = 0;
    for gender in ["F", "M"] {
        for smoking in ["non-smoker", "smoker"] {
            for age in 18..=90 {
                assumption_id += 1;
                rows.push(vec![
                    assumption_id.to_string(),
                    gender.to_string(),
                    smoking.to_string(),
                    age.to_string(),
                    micro_rate(monthly_mortality_micros(age, gender, smoking)),
                ]);
            }
        }
    }
    tables.push(Table {
        name: "mortality_assumptions",
        primary_key: "assumption_id",
        columns: vec![
            column("assumption_id", "BIGINT"),
            column("gender", "TEXT"),
            column("smoking_status", "TEXT"),
            column("age", "BIGINT"),
            column("monthly_mortality_rate", "NUMERIC(10,6)"),
        ],
        foreign_keys: vec![],
        rows,
    });

    let mut rows = Vec::new();
    assumption_id = 0;
    for product in LIFE_PRODUCTS {
        for age in 18..=70 {
            let factor: i64 = if product == "critical illness" { 3 } else { 1 };
            assumption_id += 1;
            rows.push(vec![
                assumption_id.to_string(),
                product.to_string(),
                age.to_string(),
                micro_rate((200 + (age - 18) * 10) * factor),
            ]);
        }
    }
    tables.push(Table {
        name: "morbidity_assumptions",
        primary_key: "assumption_id",
        columns: vec![
            column("assumption_id", "BIGINT"),
            column("product", "TEXT"),
            column("age", "BIGINT"),
            column("monthly_morbidity_rate", "NUMERIC(10,6)"),
        ],
        foreign_keys: vec![],
        rows,
    });

    let mut rows = Vec::new();
    assumption_id = 0;
    for product in LIFE_PRODUCTS {
        for duration in [12, 24, 36, 60, 120] {
            let base: i64 = match product {
                "term life" => 45_000,
                "whole life" => 60_000,
                "endowment" => 30_000,
                _ => 55_000,
            };
            assumption_id += 1;
            rows.push(vec![
                assumption_id.to_string(),
                product.to_string(),
                duration.to_string(),
                micro_rate(base - (duration / 12) * 1_000),
            ]);
        }
    }
    tables.push(Table {
        name: "lapse_assumptions",
        primary_key: "assumption_id",
        columns: vec![
            column("assumption_id", "BIGINT"),
            column("product", "TEXT"),
            column("duration_months", "BIGINT"),
            column("annual_lapse_rate", "NUMERIC(10,6)"),
        ],
        foreign_keys: vec![],
        rows,
    });

    let mut rows = Vec::new();
    assumption_id = 0;
    for product in LIFE_PRODUCTS {
        for months in [0, 12, 24, 60, 120] {
            let base: i64 = match product {
                "term life" => 120_000,
                "whole life" => 130_000,
                "endowment" => 180_000,
                _ => 150_000,
            };
            assumption_id += 1;
            rows.push(vec![
                assumption_id.to_string(),
                product.to_string(),
                months.to_string(),
                micro_rate(base + (months / 12) * 2_000),
            ]);
        }
    }
    tables.push(Table {
        name: "expense_assumptions",
        primary_key: "assumption_id",
        columns: vec![
            column("assumption_id", "BIGINT"),
            column("product", "TEXT"),
            column("months_in_force", "BIGINT"),
            column("expense_ratio", "NUMERIC(10,6)"),
        ],
        foreign_keys: vec![],
        rows,
    });

    let mut rows = Vec::new();
    for month in 0..=120 {
        rows.push(vec![
            month.to_string(),
            micro_rate(4_000_000 + month * 5_000),
        ]);
    }
    tables.push(Table {
        name: "discount_curve",
        primary_key: "month",
        columns: vec![
            column("month", "BIGINT"),
            column("annual_rate", "NUMERIC(10,6)"),
        ],
        foreign_keys: vec![],
        rows,
    });

    tables
}

fn schema_sql(tables: &[Table]) -> String {
    let mut out = String::new();
    out.push_str("-- Generated schema. Synthetic data only.\n");
    for t in tables.iter().rev() {
        out.push_str(&format!("DROP TABLE IF EXISTS {} CASCADE;\n", t.name));
    }
    for t in tables {
        out.push_str(&format!("CREATE TABLE {} (\n", t.name));
        let mut parts: Vec<String> = Vec::new();
        for c in &t.columns {
            let not_null = if c.not_null { " NOT NULL" } else { "" };
            parts.push(format!("    {} {}{}", c.name, c.ty, not_null));
        }
        parts.push(format!("    PRIMARY KEY ({})", t.primary_key));
        for fk in &t.foreign_keys {
            parts.push(format!(
                "    FOREIGN KEY ({}) REFERENCES {} ({})",
                fk.column, fk.parent_table, fk.parent_column
            ));
        }
        out.push_str(&parts.join(",\n"));
        out.push_str("\n);\n\n");
    }
    out
}

/// Every child row must reference an existing parent row. An empty vector means
/// the dataset is referentially sound.
fn referential_integrity_failures(tables: &[Table]) -> Vec<String> {
    let mut failures = Vec::new();
    for table in tables {
        for fk in &table.foreign_keys {
            let Some(parent) = tables.iter().find(|t| t.name == fk.parent_table) else {
                failures.push(format!(
                    "{}.{} references unknown table {}",
                    table.name, fk.column, fk.parent_table
                ));
                continue;
            };
            let Some(parent_index) = parent.column_index(fk.parent_column) else {
                failures.push(format!(
                    "{}.{} references {}.{} which is not a column",
                    table.name, fk.column, fk.parent_table, fk.parent_column
                ));
                continue;
            };
            let Some(child_index) = table.column_index(fk.column) else {
                failures.push(format!(
                    "{}.{} is declared as a foreign key but is not a column",
                    table.name, fk.column
                ));
                continue;
            };
            let parents: HashSet<&String> =
                parent.rows.iter().map(|row| &row[parent_index]).collect();
            for row in &table.rows {
                if !parents.contains(&row[child_index]) {
                    failures.push(format!(
                        "{}.{} = {} has no parent {}.{}",
                        table.name, fk.column, row[child_index], fk.parent_table, fk.parent_column
                    ));
                }
            }
        }
    }
    failures
}

fn row_count(tables: &[Table], name: &str) -> usize {
    tables
        .iter()
        .find(|t| t.name == name)
        .map(|t| t.rows.len())
        .unwrap_or(0)
}

fn duplicate_primary_keys(tables: &[Table]) -> Vec<String> {
    let mut duplicates = Vec::new();
    for table in tables {
        let Some(index) = table.column_index(table.primary_key) else {
            duplicates.push(format!(
                "{}.{} is not a column",
                table.name, table.primary_key
            ));
            continue;
        };
        let mut seen = HashSet::new();
        for row in &table.rows {
            if !seen.insert(&row[index]) {
                duplicates.push(format!(
                    "{}.{} has a repeated value {}",
                    table.name, table.primary_key, row[index]
                ));
            }
        }
    }
    duplicates
}

/// Read back a value written by `money` as integer cents. Reconciliation is done
/// in cents so no floating point is involved.
fn cents_of(value: &str) -> i64 {
    let negative = value.starts_with('-');
    let body = value.trim_start_matches('-');
    let (whole, fraction) = match body.split_once('.') {
        Some(pair) => pair,
        None => (body, "00"),
    };
    let mut fraction_cents = 0;
    for digit in fraction.chars().take(2) {
        fraction_cents = fraction_cents * 10 + (digit as i64 - 48);
    }
    if fraction.len() == 1 {
        fraction_cents *= 10;
    }
    let cents = whole.parse::<i64>().unwrap_or(0) * 100 + fraction_cents;
    if negative { -cents } else { cents }
}

/// In the banking dataset every account balance is its opening balance plus the
/// posted ledger entries against it. Pending and reversed entries do not move the
/// balance. An empty vector means every balance reconciles with its ledger.
fn unreconciled_balances(tables: &[Table]) -> Vec<String> {
    let (Some(accounts), Some(ledger)) = (
        tables.iter().find(|t| t.name == "accounts"),
        tables.iter().find(|t| t.name == "transactions"),
    ) else {
        return Vec::new();
    };
    let (Some(id_col), Some(balance_col), Some(opening_col)) = (
        accounts.column_index("account_id"),
        accounts.column_index("balance"),
        accounts.column_index("opening_balance"),
    ) else {
        return Vec::new();
    };
    let (Some(account_col), Some(amount_col), Some(direction_col), Some(status_col)) = (
        ledger.column_index("account_id"),
        ledger.column_index("amount"),
        ledger.column_index("direction"),
        ledger.column_index("status"),
    ) else {
        return Vec::new();
    };

    let mut expected: HashMap<&str, i64> = HashMap::new();
    for row in &accounts.rows {
        expected.insert(row[id_col].as_str(), cents_of(&row[opening_col]));
    }
    for row in &ledger.rows {
        if row[status_col] != "posted" {
            continue;
        }
        let amount = cents_of(&row[amount_col]);
        let entry = expected.entry(row[account_col].as_str()).or_insert(0);
        if row[direction_col] == "credit" {
            *entry += amount;
        } else {
            *entry -= amount;
        }
    }

    let mut failures = Vec::new();
    for row in &accounts.rows {
        let balance = cents_of(&row[balance_col]);
        let want = expected.get(row[id_col].as_str()).copied().unwrap_or(0);
        if balance != want {
            failures.push(format!(
                "account {} balance {} does not match its ledger ({})",
                row[id_col],
                row[balance_col],
                money(want)
            ));
        }
        if balance < 0 {
            failures.push(format!(
                "account {} balance {} is negative",
                row[id_col], row[balance_col]
            ));
        }
    }
    failures
}

fn write_dataset(out: &Path, label: &str, tables: &[Table]) -> std::io::Result<()> {
    let dir = out.join(label);
    fs::create_dir_all(&dir)?;
    let csv_dir = dir.join("csv");
    fs::create_dir_all(&csv_dir)?;

    let mut seed = String::new();
    seed.push_str("-- Generated data. Same seed and counts produce the same file.\n");
    for t in tables {
        seed.push_str(&t.sql());
        fs::write(csv_dir.join(format!("{}.csv", t.name)), t.csv())?;
    }
    fs::write(dir.join("schema.sql"), schema_sql(tables))?;
    fs::write(dir.join("seed.sql"), seed)?;

    let mut manifest = String::from("file,table,rows,columns\n");
    for t in tables {
        manifest.push_str(&format!(
            "csv/{}.csv,{},{},\"{}\"\n",
            t.name,
            t.name,
            t.rows.len(),
            t.declared_columns()
        ));
    }
    fs::write(dir.join("manifest.csv"), manifest)?;
    Ok(())
}

/// Generation stops if the dataset would break its own constraints.
fn check_integrity(tables: &[Table], label: &str) {
    let failures = referential_integrity_failures(tables);
    let duplicates = duplicate_primary_keys(tables);
    let balances = unreconciled_balances(tables);
    if failures.is_empty() && duplicates.is_empty() && balances.is_empty() {
        let accounts = row_count(tables, "accounts");
        println!(
            "{label}: {} foreign key relationships and {} primary keys verified{}",
            tables.iter().map(|t| t.foreign_keys.len()).sum::<usize>(),
            tables.len(),
            if accounts > 0 {
                format!(
                    ", and {} account balances reconcile with their ledger",
                    accounts
                )
            } else {
                String::new()
            }
        );
        return;
    }
    for message in failures
        .iter()
        .chain(duplicates.iter())
        .chain(balances.iter())
        .take(10)
    {
        eprintln!("error: {message}");
    }
    eprintln!("error: the generated {label} dataset is not referentially sound");
    std::process::exit(1);
}

fn main() {
    let cli = parse_cli();
    fs::create_dir_all(&cli.out).unwrap_or_else(|e| {
        eprintln!("error: cannot create {}: {e}", cli.out.display());
        std::process::exit(1);
    });
    // A fixed base day keeps the date window stable across runs.
    let base_day = 19_700;

    if cli.dataset == "retail_banking" || cli.dataset == "both" || cli.dataset == "all" {
        let mut rng = Rng::new(cli.seed ^ 0x62_61_6e);
        let tables = retail_banking_tables(&cli, &mut rng, base_day);
        check_integrity(&tables, "retail_banking");
        write_dataset(&cli.out, "retail_banking", &tables).unwrap_or_else(|e| {
            eprintln!("error: writing the banking dataset: {e}");
            std::process::exit(1);
        });
        println!(
            "retail_banking: {} tables, {} accounts, {} transactions, {} loans, {} customers",
            tables.len(),
            row_count(&tables, "accounts"),
            row_count(&tables, "transactions"),
            row_count(&tables, "loans"),
            row_count(&tables, "customers")
        );
    }

    if cli.dataset == "non_life" || cli.dataset == "both" || cli.dataset == "all" {
        let mut rng = Rng::new(cli.seed ^ 0x69_6e_73);
        let tables = non_life_tables(&cli, &mut rng, base_day);
        check_integrity(&tables, "non_life");
        write_dataset(&cli.out, "non_life", &tables).unwrap_or_else(|e| {
            eprintln!("error: writing the insurance dataset: {e}");
            std::process::exit(1);
        });
        println!(
            "non_life: {} tables, {} policies, {} claims, {} coverages, {} endorsements",
            tables.len(),
            row_count(&tables, "policies"),
            row_count(&tables, "claims"),
            row_count(&tables, "coverages"),
            row_count(&tables, "endorsements")
        );
    }

    if cli.dataset == "life" || cli.dataset == "all" {
        // A separate random stream, so generating one dataset never shifts another.
        let mut rng = Rng::new(cli.seed ^ 0x6c_69_66);
        let tables = life_tables(&cli, &mut rng, base_day);
        check_integrity(&tables, "life");
        write_dataset(&cli.out, "life", &tables).unwrap_or_else(|e| {
            eprintln!("error: writing the life dataset: {e}");
            std::process::exit(1);
        });
        println!(
            "life: {} tables, {} policies, {} mortality rows, {} discount points",
            tables.len(),
            row_count(&tables, "policies"),
            row_count(&tables, "mortality_assumptions"),
            row_count(&tables, "discount_curve")
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn epoch_is_1970_01_01() {
        assert_eq!(date_from_days(0), "1970-01-01");
    }

    #[test]
    fn known_dates_round_trip() {
        assert_eq!(date_from_days(19723), "2024-01-01");
        assert_eq!(date_from_days(19723 + 366), "2025-01-01"); // 2024 is a leap year
        assert_eq!(date_from_days(19723 + 60), "2024-03-01");
    }

    #[test]
    fn money_is_exact_and_two_decimals() {
        assert_eq!(money(1234567), "12345.67");
        assert_eq!(money(-500), "-5.00");
        assert_eq!(money(0), "0.00");
    }

    #[test]
    fn csv_and_sql_agree_on_every_cell() {
        let table = Table {
            name: "claims",
            primary_key: "claim_id",
            columns: vec![
                column("claim_id", "BIGINT"),
                column("policy_id", "BIGINT"),
                column("status", "TEXT"),
                column("claimed_amount", "NUMERIC(12,2)"),
            ],
            foreign_keys: vec![foreign_key("policy_id", "policies", "policy_id")],
            rows: vec![
                vec!["1".into(), "7".into(), "open".into(), "12.50".into()],
                vec!["2".into(), "8".into(), "a, b".into(), "0.00".into()],
            ],
        };
        let sql = table.sql();
        let csv = table.csv();
        assert!(sql.contains("(1, 7, 'open', 12.50)"));
        assert!(sql.contains("'a, b'"));
        assert!(csv.starts_with("claim_id,policy_id,status,claimed_amount\n"));
        assert!(csv.contains("\"a, b\""));
        assert!(!sql.contains("''a, b''"));
    }

    #[test]
    fn generated_rows_are_well_formed() {
        let mut rng = Rng::new(1);
        let cli = Cli {
            out: PathBuf::from("out"),
            seed: 1,
            dataset: "both".to_string(),
            customers: 10,
            transactions: 20,
            loans: 3,
            policyholders: 5,
            policies: 6,
            claims: 8,
            life_policyholders: 4,
            life_policies: 6,
        };
        for table in retail_banking_tables(&cli, &mut rng, 19_700) {
            for row in &table.rows {
                assert_eq!(
                    row.len(),
                    table.columns.len(),
                    "{} row width mismatch",
                    table.name
                );
                for (index, value) in row.iter().enumerate() {
                    if table.columns[index].not_null {
                        assert!(!value.is_empty(), "{} has an empty cell", table.name);
                    }
                }
            }
        }
    }

    #[test]
    fn every_foreign_key_resolves() {
        let cli = small_cli();
        let mut rng = Rng::new(7);
        for (label, tables) in [
            (
                "retail_banking",
                retail_banking_tables(&cli, &mut rng, 19_700),
            ),
            ("non_life", non_life_tables(&cli, &mut rng, 19_700)),
            ("life", life_tables(&cli, &mut rng, 19_700)),
        ] {
            let failures = referential_integrity_failures(&tables);
            assert!(failures.is_empty(), "{label}: {failures:?}");
        }
    }

    #[test]
    fn primary_keys_are_unique() {
        let cli = small_cli();
        let mut rng = Rng::new(11);
        for (label, tables) in [
            (
                "retail_banking",
                retail_banking_tables(&cli, &mut rng, 19_700),
            ),
            ("non_life", non_life_tables(&cli, &mut rng, 19_700)),
            ("life", life_tables(&cli, &mut rng, 19_700)),
        ] {
            let duplicates = duplicate_primary_keys(&tables);
            assert!(duplicates.is_empty(), "{label}: {duplicates:?}");
        }
    }

    #[test]
    fn account_balances_reconcile_with_the_ledger() {
        let cli = small_cli();
        let mut rng = Rng::new(23);
        let mut tables = retail_banking_tables(&cli, &mut rng, 19_700);
        let failures = unreconciled_balances(&tables);
        assert!(failures.is_empty(), "banking: {failures:?}");

        // A balance that disagrees with the ledger has to be caught.
        let accounts = tables
            .iter_mut()
            .find(|t| t.name == "accounts")
            .expect("accounts table");
        let balance = accounts.column_index("balance").expect("balance column");
        accounts.rows[0][balance] = money(cents_of(&accounts.rows[0][balance]) + 100);
        let failures = unreconciled_balances(&tables);
        assert_eq!(failures.len(), 1, "{failures:?}");
        assert!(failures[0].contains("does not match its ledger"));
    }

    #[test]
    fn posted_entries_only_move_a_balance() {
        let cli = small_cli();
        let mut rng = Rng::new(31);
        let tables = retail_banking_tables(&cli, &mut rng, 19_700);
        let accounts = tables.iter().find(|t| t.name == "accounts").unwrap();
        let ledger = tables.iter().find(|t| t.name == "transactions").unwrap();
        let opening = accounts.column_index("opening_balance").unwrap();
        let balance = accounts.column_index("balance").unwrap();
        let account_col = ledger.column_index("account_id").unwrap();
        let amount = ledger.column_index("amount").unwrap();
        let direction = ledger.column_index("direction").unwrap();
        let status = ledger.column_index("status").unwrap();
        let id_col = accounts.column_index("account_id").unwrap();
        for row in &accounts.rows {
            let mut want = cents_of(&row[opening]);
            for entry in &ledger.rows {
                if entry[account_col] != row[id_col] || entry[status] != "posted" {
                    continue;
                }
                let value = cents_of(&entry[amount]);
                if entry[direction] == "credit" {
                    want += value;
                } else {
                    want -= value;
                }
            }
            assert_eq!(cents_of(&row[balance]), want, "account {}", row[id_col]);
            assert!(
                cents_of(&row[balance]) >= 0,
                "account {} is overdrawn",
                row[id_col]
            );
        }
    }

    #[test]
    fn money_strings_round_trip_through_cents() {
        for value in [0, 100, 1234567, -500, 999_999_999] {
            assert_eq!(cents_of(&money(value)), value, "cents_of(money({value}))");
        }
        assert_eq!(cents_of("0.50"), 50);
        assert_eq!(cents_of("1000000.01"), 100_000_001);
    }

    #[test]
    fn a_broken_foreign_key_is_caught() {
        let mut table = Table {
            name: "claims",
            primary_key: "claim_id",
            columns: vec![column("claim_id", "BIGINT"), column("policy_id", "BIGINT")],
            foreign_keys: vec![foreign_key("policy_id", "policies", "policy_id")],
            rows: vec![vec!["1".into(), "999".into()]],
        };
        let parents = Table {
            name: "policies",
            primary_key: "policy_id",
            columns: vec![column("policy_id", "BIGINT")],
            foreign_keys: vec![],
            rows: vec![vec!["1".into()], vec!["2".into()]],
        };
        let failures = referential_integrity_failures(&[parents, table.clone()]);
        assert_eq!(failures.len(), 1);
        assert!(failures[0].contains("claims.policy_id = 999 has no parent policies.policy_id"));
        table.rows[0][1] = "2".into();
        let parents = Table {
            name: "policies",
            primary_key: "policy_id",
            columns: vec![column("policy_id", "BIGINT")],
            foreign_keys: vec![],
            rows: vec![vec!["1".into()], vec!["2".into()]],
        };
        assert!(referential_integrity_failures(&[parents, table]).is_empty());
    }

    #[test]
    fn no_column_name_is_a_postgres_reserved_word() {
        // Words Postgres will not accept as a bare identifier. The generator used
        // to put `limit` in the coverages table and the schema failed to load.
        const RESERVED: [&str; 24] = [
            "all", "and", "any", "as", "asc", "case", "check", "column", "desc", "distinct",
            "else", "end", "false", "from", "group", "having", "if", "in", "limit", "not", "null",
            "order", "select", "true",
        ];
        let cli = small_cli();
        let mut rng = Rng::new(13);
        for tables in [
            retail_banking_tables(&cli, &mut rng, 19_700),
            non_life_tables(&cli, &mut rng, 19_700),
            life_tables(&cli, &mut rng, 19_700),
        ] {
            for table in &tables {
                for c in &table.columns {
                    assert!(
                        !RESERVED.contains(&c.name),
                        "column {} uses a reserved word",
                        c.name
                    );
                }
            }
        }
    }

    #[test]
    fn life_underwriting_inputs_are_in_range() {
        let cli = small_cli();
        let mut rng = Rng::new(23);
        let tables = life_tables(&cli, &mut rng, 19_700);
        assert!(referential_integrity_failures(&tables).is_empty());
        for table in &tables {
            let index = table.column_index(table.primary_key).unwrap();
            for row in &table.rows {
                assert!(!row[index].is_empty(), "{} has an empty key", table.name);
            }
        }
        let mortality = tables
            .iter()
            .find(|t| t.name == "mortality_assumptions")
            .unwrap();
        for row in &mortality.rows {
            let rate: f64 = row[4].parse().unwrap();
            let age: i64 = row[3].parse().unwrap();
            assert!((18..=90).contains(&age), "mortality age {age} out of range");
            assert!(
                rate > 0.0 && rate < 0.05,
                "implausible mortality rate {rate}"
            );
        }
        let policies = tables.iter().find(|t| t.name == "policies").unwrap();
        for row in &policies.rows {
            let sum_assured: f64 = row[11].parse().unwrap();
            let premium: f64 = row[12].parse().unwrap();
            let guaranteed: f64 = row[15].parse().unwrap();
            assert!(
                premium > 0.0 && premium < sum_assured,
                "premium {premium} vs sum assured {sum_assured}"
            );
            assert!(
                guaranteed <= sum_assured,
                "guaranteed {guaranteed} exceeds sum assured {sum_assured}"
            );
            assert!(
                row[10] == "NULL" || row[10].parse::<i64>().is_ok(),
                "bad term_years {}",
                row[10]
            );
        }
    }

    #[test]
    fn schema_declares_keys_and_types() {
        let cli = small_cli();
        let mut rng = Rng::new(17);
        let tables = non_life_tables(&cli, &mut rng, 19_700);
        let schema = schema_sql(&tables);
        assert!(schema.contains("CREATE TABLE claims ("));
        assert!(schema.contains("    PRIMARY KEY (claim_id)"));
        assert!(schema.contains("    FOREIGN KEY (policy_id) REFERENCES policies (policy_id)"));
        assert!(schema.contains("    decision_date DATE"));
        assert!(schema.contains("    claim_id BIGINT NOT NULL"));
        assert!(schema.contains("    claimed_amount NUMERIC(12,2) NOT NULL"));
        assert!(!schema.contains("    limit NUMERIC"));
    }

    fn small_cli() -> Cli {
        Cli {
            out: PathBuf::from("out"),
            seed: 7,
            dataset: "both".to_string(),
            customers: 8,
            transactions: 20,
            loans: 3,
            policyholders: 5,
            policies: 6,
            claims: 8,
            life_policyholders: 4,
            life_policies: 6,
        }
    }
}
