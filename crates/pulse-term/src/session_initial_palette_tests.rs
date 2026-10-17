use super::*;

#[tokio::test]
async fn initial_palette_precedes_first_osc_reply_and_survives_remote_reset()
-> Result<(), Box<dyn std::error::Error>> {
    let (transport, mut driver) = ChannelTransport::bounded_driver();
    let palette = crate::QueryPalette {
        foreground: [0x12, 0x34, 0x56],
        ..Default::default()
    };
    let (handle, task) = spawn_session_with_palette(TermSize::new(40, 5)?, 5, transport, palette);
    for (bytes, expected) in [
        (b"\x1b]10;?\x07".as_slice(), "1212/3434/5656"),
        (
            b"\x1b]10;#010203\x07\x1b]10;?\x07".as_slice(),
            "0101/0202/0303",
        ),
        (b"\x1b]110\x07\x1b]10;?\x07".as_slice(), "1212/3434/5656"),
    ] {
        driver.output.send(bytes.to_vec()).await?;
        let command = tokio::time::timeout(std::time::Duration::from_secs(2), driver.input.recv())
            .await?
            .ok_or("missing reply")?;
        let TransportWrite::Reply(reply) = command else {
            return Err("unexpected transport write".into());
        };
        let reply = std::str::from_utf8(&reply)?;
        assert!(reply.starts_with("\x1b]10;"));
        assert!(reply.contains(expected), "unexpected color reply");
    }
    handle.close();
    task.join().await?;
    Ok(())
}
