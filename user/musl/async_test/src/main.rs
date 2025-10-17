use tokio::{signal, task::yield_now, time};

async fn say_world() {
    println!("world");
}

#[cfg(feature = "tokio")]
#[tokio::main]
async fn main() {
    tokio::spawn(async move {
        let mut now = time::Instant::now();
        loop {
            let new_now = time::Instant::now();
            let duration = new_now.duration_since(now);
            if duration.as_secs() >= 2 {
                // open a file to trigger the tracepoint
                // println!("Triggering tracepoint by opening /bin");
                let bin = std::fs::File::open("/bin").unwrap();
                drop(bin);
                now = new_now;
            }
            yield_now().await;
        }
    });
    let ctrl_c = signal::ctrl_c();
    println!("Waiting for Ctrl-C...");
    ctrl_c.await.unwrap();
    println!("Exiting...");
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
