# winspawn

Spawn process for passing Universal CRT's file descriptor on windows.

Using `_spawn` & `_dup`.

## Example

```rust
use winspawn::{move_fd, spawn, FileDescriptor};
use std::mem;
fn main() {
    let stdout = unsafe { FileDescriptor::from_raw_fd(1) };
    // copy stdout(1) to 3
    let proc = move_fd(&stdout, 3, |_| {
        // print fd 3 stat
        spawn("python", ["-c", "import os; print(os.stat(3))"])
    }).unwrap();
    mem::forget(stdout); // suppress close on drop.

    let exit_code = proc.wait();
    assert_eq!(0, exit_code);
}
```
