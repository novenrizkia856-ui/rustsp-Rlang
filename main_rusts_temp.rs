



struct Wallet {
    id: u32,
    balance: i64,
}

enum Tx {
    Deposit { id: u32, amount: i64 },
    Withdraw { id: u32, amount: i64 },
    Query(u32),
}




fn classify(balance: i64) -> String {
    if balance >= 1000 {
        String::from("gold")
    } else if balance >= 0 {
        String::from("normal")
    } else {
        String::from("debt")
    }
}




fn apply_tx(w: Wallet, tx: Tx) -> effects(write w) Wallet {
    match tx {
        Tx::Deposit { id, amount } => (
            if w.id == id {
                let mut next = w;
                next.balance = next.balance + amount;
                next
            } else {
                w
            }
        ),

        Tx::Withdraw { id, amount } => (
            if w.id == id {
                let mut next = w;
                next.balance = next.balance - amount;
                next
            } else {
                w
            }
        ),

        Tx::Query(_) => {
            w
        },
    }
}




fn process_all(w: Wallet, txs: [Tx]) -> effects(write w) Wallet {
    let mut acc = w;
    let mut i = 0;

    while i < 3 {
        acc = apply_tx(acc, txs[i]);
        i = i + 1;
    }

    acc
}




fn log_status(label: String, value: i64) -> effects(io) () {
    println("{} = {}", label, value)
}




fn main() -> effects(io) () {

    let mut account = Wallet {
        id: 7,
        balance: 100,
    };

    effect write(account);

    let txs = [
        Tx::Deposit {id: 7, amount: 500},
        Tx::Withdraw {id: 7, amount: 200},
        Tx::Deposit {id: 7, amount: 900},
    ];


    account = process_all(account, txs);

    let tier = classify(account.balance);

    log_status(String::from("balance"), account.balance);

    match tier.as_str() {
        "gold" => {
            log_status(String::from("tier"), 1)
        },
        "normal" => {
            log_status(String::from("tier"), 2)
        },
        _ => {
            log_status(String::from("tier"), 3)
        },
    }
}