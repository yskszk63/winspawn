# winspawn

Spawn process for passing Universal CRT's file descriptor on windows.

Using `_spawn` & `_dup`.

## Example

```rust
use winspawn::{move_fd, spawn, FileDescriptor, Mode};

use std::mem;
use std::io;
use std::fs;
use std::os::windows::io::IntoRawHandle;

fn main() -> io::Result<()> {
    let file = fs::File::open("Cargo.toml")?;
    let handle = file.into_raw_handle();
    let fd = FileDescriptor::from_raw_handle(handle, Mode::ReadOnly)?;

    let mut proc = move_fd(&fd, 3, |_| {
        // print fd 3 stat
        spawn("python", ["-c", r#""import os; print(os.stat(3))""#])
    })?;

    let exit_code = proc.wait()?;
    assert_eq!(0, exit_code);

    Ok(())
}
```

License: MIT/Apache-2.0
