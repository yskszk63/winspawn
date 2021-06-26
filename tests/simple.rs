use std::io;
use winspawn::spawn;

#[test]
fn test_simple() -> io::Result<()> {
    let mut prog = spawn("cargo", ["--version"])?;
    let exitcode = prog.wait()?;
    assert_eq!(0, exitcode);

    Ok(())
}
