


struct Wallet {
id: u32,
balance: i64,
}

enum Transaction {
Deposit { amount: i64 },
Withdraw { amount: i64 },
}


fn apply_tx(w: Wallet, tx: Transaction) -> Wallet {
match tx {
Transaction::Deposit { amount } => {
Wallet {
id: w.id.clone(),
balance: w.balance + amount,
}
},
Transaction::Withdraw { amount } => {
Wallet {
id: w.id.clone(),
balance: w.balance - amount,
}
},
}
}


fn print_balance(w: &Wallet) {
println!("Wallet #{}: Balance = {}", w.id, w.balance);
}

fn main() {

let wallet = Wallet { id: 1, balance: 100 };
print_balance(&wallet);


let tx1 = Transaction::Deposit { amount: 50 };
let wallet = apply_tx(wallet.clone(), tx1.clone());
println!("After deposit:");
print_balance(&wallet);


let tx2 = Transaction::Withdraw { amount: 30 };
let wallet = apply_tx(wallet.clone(), tx2.clone());
println!("After withdrawal:");
print_balance(&wallet);
}