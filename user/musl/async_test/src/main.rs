async fn say_world() {
    println!("world");
}

#[cfg(feature = "tokio")]
#[tokio::main]
async fn main() {
    // Calling `say_world()` does not execute the body of `say_world()`.
    let op = say_world();

    // This println! comes first
    println!("hello");

    // Calling `.await` on `op` starts executing `say_world`.
    op.await;
}

#[cfg(feature = "smol")]
fn main() {
    smol::block_on(async {
        // Calling `say_world()` does not execute the body of `say_world()`.
        let op = say_world();

        // This println! comes first
        println!("hello");

        // Calling `.await` on `op` starts executing `say_world`.
        op.await;
    });
}
