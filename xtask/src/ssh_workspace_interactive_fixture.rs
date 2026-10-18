//! Synthetic keyboard-interactive peer for production WebView acceptance only.
use super::*;
use std::borrow::Cow;

pub(super) fn respond(
    peer: &mut GuiPeer,
    user: &str,
    response: Option<server::Response<'_>>,
) -> Result<server::Auth, russh::Error> {
    let Some(round) = peer.interactive_round else {
        return Ok(server::Auth::reject());
    };
    if user != "user" {
        return Ok(server::Auth::reject());
    }
    match (round, response) {
        (0, None) => {
            peer.interactive_round = Some(1);
            eprintln!("fixture.ssh_interactive_round_one");
            Ok(server::Auth::Partial {
                name: Cow::Borrowed("Fixture <literal> service"),
                instructions: Cow::Borrowed("Synthetic acceptance prompts only"),
                prompts: Cow::Owned(vec![
                    (Cow::Borrowed("Account <literal>"), true),
                    (Cow::Borrowed("First verification code"), false),
                ]),
            })
        }
        (1, Some(mut answers)) => {
            let exact = answers
                .next()
                .is_some_and(|value| value.as_ref() == b"user")
                && answers
                    .next()
                    .is_some_and(|value| value.as_ref() == b"123456")
                && answers.next().is_none();
            if !exact {
                return Ok(server::Auth::reject());
            }
            peer.interactive_round = Some(2);
            eprintln!("fixture.ssh_interactive_round_two");
            Ok(server::Auth::Partial {
                name: Cow::Borrowed("Fixture <literal> service"),
                instructions: Cow::Borrowed("Synthetic second round"),
                prompts: Cow::Owned(vec![(Cow::Borrowed("Second verification code"), false)]),
            })
        }
        (2, Some(mut answers)) => {
            eprintln!("fixture.ssh_interactive_second_answer_received");
            if answers
                .next()
                .is_none_or(|value| value.as_ref() != b"654321")
                || answers.next().is_some()
            {
                return Ok(server::Auth::reject());
            }
            peer.interactive_round = Some(3);
            peer.accepted = true;
            peer.auth.auth.fetch_add(1, Ordering::SeqCst);
            peer.auth.exact.store(true, Ordering::SeqCst);
            eprintln!("fixture.ssh_interactive_authenticated");
            Ok(server::Auth::Accept)
        }
        _ => Ok(server::Auth::reject()),
    }
}
