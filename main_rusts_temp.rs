






fn add(a: i32, b: i32) -> i32 {
    a + b
}

fn factorial(n: i32) -> i32 {
    if n <= 1 {
        1
    } else {
        n * factorial(n - 1)
    }
}





fn say_hello(name: String) {
    println!("Hello, {}!", name);
}

fn show_num(n: i32) {
    println!("Number: {}", n);
}





#[derive(Clone)]
struct Counter {
    value: i32,,
    name: String,,
}

fn new_counter(name: String) -> Counter {
    Counter { value: 0, name: name }
}

fn print_counter(ctr: Counter) {
    println!("{}: {}", ctr.name, ctr.value);
}





fn count_up() -> i32 {
    let mut cnt = 0;
    cnt = cnt + 1;
    cnt = cnt + 1;
    cnt = cnt + 1;
    cnt
}

fn sum_to_n(n: i32) -> i32 {
    let mut total = 0;
    let mut idx = 0;
    while idx <= n {
        total = total + &idx;
        idx = idx + 1;
    }
    total
}





fn test_outer_mutation() -> i32 {
    let mut result = 0;
    {
        result = 42;
    }
    result
}

fn nested_outer() -> i32 {
    let mut val = 0;
    {
        {
            val = 100;
        }
    }
    val
}





fn max_val(a: i32, b: i32) -> i32 {
    if a > b {
        a
    } else {
        b
    }
}

fn sign_of(n: i32) -> i32 {
    if n < 0 {
        -1
    } else if n > 0 {
        1
    } else {
        0
    }
}

fn abs_val(n: i32) -> i32 {
    if n < 0 {
        -n
    } else {
        n
    }
}





fn greet_and_count(name: String) -> i32 {
    println!("Hello, {}!", name);
    42
}





fn main() {
    println!("=== RustS+ Test Suite ===");


    println!("\n--- Pure Functions ---");
    let sum = add(10, 20);
    println!("add(10, 20) = {}", sum);

    let fact = factorial(5);
    println!("factorial(5) = {}", fact);


    println!("\n--- I/O Effects ---");
    say_hello(String::from("RustS+"));
    show_num(123);


    println!("\n--- Structs ---");
    let ctr = new_counter(String::from("MyCounter"));
    print_counter(ctr);


    println!("\n--- Mutation with mut ---");
    let cnt_result = count_up();
    println!("count_up() = {}", cnt_result);

    let sum_result = sum_to_n(10);
    println!("sum_to_n(10) = {}", sum_result);


    println!("\n--- Outer Keyword ---");
    let outer_result = test_outer_mutation();
    println!("test_outer_mutation() = {}", outer_result);

    let nested_result = nested_outer();
    println!("nested_outer() = {}", nested_result);


    println!("\n--- If Expression Completeness ---");
    let max_result = max_val(15, 10);
    println!("max_val(15, 10) = {}", max_result);

    let sign_neg = sign_of(-5);
    let sign_pos = sign_of(5);
    let sign_zero = sign_of(0);
    println!("sign_of(-5) = {}", sign_neg);
    println!("sign_of(5) = {}", sign_pos);
    println!("sign_of(0) = {}", sign_zero);

    let abs_result = abs_val(-42);
    println!("abs_val(-42) = {}", abs_result);


    println!("\n--- Combined Effects ---");
    let greet_result = greet_and_count(String::from("World"));
    println!("greet_and_count returned: {}", greet_result);

    println!("\n=== All Tests Completed! ===");
}