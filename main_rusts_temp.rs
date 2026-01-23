












fn pure_add(a: i32, b: i32) -> i32 {
a + b
}


fn fibonacci(n: u64) -> u64 {
if n <= 1 {
n
} else {
fibonacci(n - 1) + fibonacci(n - 2)
}
}


fn create_point(x: i32, y: i32) -> Point {
Point { x: x, y: y }
}






fn log_message(msg: String) {
println!("{}", msg);
}


fn interactive_greeting(name: String) {
println!("Hello, {}!", name);
println!("Welcome to RustS+!");
}


fn prompt_and_log(question: String) -> String {
println!("{}", question);
question
}






fn create_empty_vec() -> Vec<i32> {
Vec::new()
}


fn create_vec_with_capacity(cap: usize) -> Vec<String> {
Vec::with_capacity(cap)
}


fn string_from_str(s: &str) -> String {
String::from(s)
}


fn boxed_value(x: i32) -> Box<i32> {
Box::new(x)
}


fn create_collections() -> (Vec<i32>, String, HashMap<String, i32>) {
let items = Vec::new();
let name = String::from("test");
let map = HashMap::new();
(items, name, map)
}


fn number_to_string(n: i32) -> String {
n.to_string()
}






fn unsafe_unwrap(opt: Option<i32>) -> i32 {
opt.unwrap()
}


fn unsafe_expect(opt: Option<String>) -> String {
opt.expect("Value required!")
}


fn assert_positive(n: i32) {
assert!(n > 0);
}


fn dangerous_operations(a: Option<i32>, b: Option<i32>) -> i32 {
let x = a.unwrap();
let y = b.expect("b must exist");
assert!(x > 0);
x + y
}






fn get_balance(acc: Account) -> i64 {
acc.balance
}


fn calculate_total(acc1: Account, acc2: Account) -> i64 {
acc1.balance + acc2.balance
}


fn compute_interest(acc: Account, rate: f64) -> f64 {
acc.balance as f64 * rate
}






fn deposit(acc: Account, amount: i64) -> Account {
acc.balance += amount;
acc
}


fn transfer_internal(from: Account, to: Account, amount: i64) {
from.balance -= amount;
to.balance += amount;
}


fn withdraw_safe(acc: Account, amount: i64) -> Account {
assert!(acc.balance >= amount);
acc.balance -= amount;
acc
}






fn format_and_log(data: i32) -> String {
let msg = format!("Data: {}", data);
println!("{}", msg);
msg
}


fn log_required(opt: Option<String>) {
let msg = opt.unwrap();
println!("{}", msg);
}


fn create_or_fail(should_create: bool) -> Vec<i32> {
assert!(should_create);
Vec::new()
}


fn double_balance(acc: Account) -> Account {
let current = acc.balance;
acc.balance = current * 2;
acc
}


fn complex_operation(acc: Account, name: String) -> String {
assert!(acc.balance > 0);
acc.balance -= 1;
let result = format!("Processed: {}", name);
println!("{}", result);
result
}







fn wrapper_log(msg: String) {
log_message(msg.clone());
}


fn wrapper_alloc() -> Vec<i32> {
create_empty_vec()
}


fn combined_wrapper(msg: String) -> Vec<i32> {
log_message(msg.clone());
create_empty_vec()
}



fn deep_call_chain(acc: Account, amount: i64) -> Account {
let result = deposit(acc.clone(), amount);
log_message("Deposit complete".to_string());
result
}





#[derive(Clone)]
struct Point {
x: i32,
y: i32,
}

#[derive(Clone)]
struct Account {
id: u64,
balance: i64,
owner: String,
}

#[derive(Clone)]
struct Wallet {
accounts: Vec<Account>,
total_balance: i64,
}





impl Account {

fn new(id: u64, owner: String) -> Account {
Account {
id: id,
balance: 0,
owner: owner,
}
}


fn get_id(self) -> u64 {
self.id
}


fn add_funds(self, amount: i64) {
self.balance += amount;
}


fn print_balance(self) {
println!("Balance: {}", self.balance);
}
}

impl Wallet {

fn new() -> Wallet {
Wallet {
accounts: Vec::new(),
total_balance: 0,
}
}


fn add_account(self, acc: Account) {
self.total_balance += acc.balance;
self.accounts.push(acc);
}
}






fn map_accounts(accounts: Vec<Account>, f: fn(Account) Account) -> Vec<Account> {
let result = Vec::new();
for acc in accounts {
result.push(f(acc));
}
result
}


fn log_all_balances(accounts: Vec<Account>) {
for acc in accounts {
println!("Account {}: {}", acc.id, acc.balance);
}
}







fn safe_divide(a: i32, b: i32) -> Result<i32, String> {
if b == 0 {
Err(String::from("Division by zero"))
} else {
Ok(a / b)
}
}


fn calculate_ratio(x: i32, y: i32, z: i32) -> Result<i32, String> {
let first = safe_divide(x, y)?;
safe_divide(first, z)
}


fn process_data(input: Option<i32>) -> Result<i32, String> {
let value = input.ok_or(String::from("No input"))?;
Ok(value * 2)
}






fn main() {

let sum = pure_add(10, 20);
let fib = fibonacci(10);


log_message(String::from("Starting RustS+ Effect Test"));


let numbers = create_empty_vec();
let name = string_from_str("RustS+");


let acc = Account::new(1, String::from("Alice"));
acc.add_funds(1000);
acc.print_balance();


let result = complex_operation(acc.clone(), String::from("test"));

println!("Test completed successfully!");
}