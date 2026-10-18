//! Password partial-success policies for the owned native acceptance fixture only.
use super::*;
pub(super) fn authenticate(
    peer: &mut GuiPeer,
    user: &str,
    password: &str,
    policy: &std::path::Path,
) -> server::Auth {
    eprintln!("fixture.ssh_password_joint_auth_attempt");
    if user != "user"
        || password != "fixed-target-secret"
        || peer.password_factor_complete
        || peer.interactive_round != Some(0)
    {
        eprintln!("fixture.ssh_password_joint_rejected");
        return server::Auth::reject();
    }
    match std::fs::read_to_string(policy).as_deref() {
        Ok("partial") => {
            peer.password_factor_complete = true;
            eprintln!("fixture.ssh_password_joint_partial_success");
            server::Auth::Reject {
                proceed_with_methods: Some((&[russh::MethodKind::KeyboardInteractive][..]).into()),
                partial_success: true,
            }
        }
        Ok("partial-no-mfa") => {
            peer.password_factor_complete = true;
            eprintln!("fixture.ssh_password_joint_missing_mfa");
            server::Auth::Reject {
                proceed_with_methods: Some((&[russh::MethodKind::Password][..]).into()),
                partial_success: true,
            }
        }
        Ok("complete") => {
            peer.accepted = true;
            eprintln!("fixture.ssh_password_joint_complete_success");
            server::Auth::Accept
        }
        _ => {
            eprintln!("fixture.ssh_password_joint_rejected");
            server::Auth::reject()
        }
    }
}
