use std::io;
use winspawn::spawn;

#[test]
fn test_simple() -> io::Result<()> {
    let mut prog = spawn(r#"C:\Rust\.cargo\bin\cargo.exe"#, ["--version"])?;
    let exitcode = prog.wait()?;
    assert_eq!(0, exitcode);

    Ok(())
}
