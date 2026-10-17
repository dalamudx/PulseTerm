//! Only fixed synthetic answers for the owned loopback three-hop fixture.
use super::Jump;
use russh::server;
use std::borrow::Cow;
use zeroize::Zeroizing;

pub(super) fn answer(
    peer: &mut Jump,
    user: &str,
    response: Option<server::Response<'_>>,
) -> Result<server::Auth, russh::Error> {
    let Some(round) = peer.interactive_round else {
        return Ok(server::Auth::reject());
    };
    if user != "user" {
        return Ok(server::Auth::reject());
    }
    let answers = response.map(|response| {
        response
            .map(|answer| Zeroizing::new(answer.to_vec()))
            .collect::<Vec<_>>()
    });
    let prompts = match (round, answers) {
        (0, None) => {
            peer.interactive_round = Some(1);
            vec![
                ("Account <literal>".into(), true),
                ("First verification code".into(), false),
            ]
        }
        (1, Some(answers))
            if answers.len() == 2
                && answers[0].as_slice() == b"user"
                && answers[1].as_slice() == b"123456" =>
        {
            peer.interactive_round = Some(2);
            vec![("Second verification code".into(), false)]
        }
        (2, Some(answers)) if answers.len() == 1 && answers[0].as_slice() == b"654321" => {
            peer.interactive_round = Some(3);
            eprintln!("fixture.route.{}.interactive_authenticated", peer.label);
            eprintln!("fixture.route.{}.authenticated", peer.label);
            return Ok(server::Auth::Accept);
        }
        _ => {
            peer.interactive_round = Some(u8::MAX);
            eprintln!("fixture.route.{}.interactive_rejected", peer.label);
            return Ok(server::Auth::reject());
        }
    };
    eprintln!(
        "fixture.route.{}.interactive_round.{}",
        peer.label,
        peer.interactive_round.unwrap_or(u8::MAX)
    );
    Ok(server::Auth::Partial {
        name: Cow::Borrowed("Owned jump MFA <literal>"),
        instructions: Cow::Borrowed("Only synthetic fixture answers"),
        prompts: Cow::Owned(prompts),
    })
}
