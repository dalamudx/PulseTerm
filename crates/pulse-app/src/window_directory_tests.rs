#![allow(clippy::unwrap_used)]
use super::*;
fn ready(directory: &WindowDirectory, registration: &WindowRegistration) {
    let documents = directory.documents();
    documents.started(registration.owner()).unwrap();
    let nonce = documents.finished(registration.owner()).unwrap();
    documents.confirm(registration.owner(), nonce).unwrap();
}
#[test]
fn labels_are_canonical_metadata_and_unknown_labels_never_route() {
    let directory = WindowDirectory::new(SessionRegistry::new());
    assert!(WindowLabel::parse("workspace").is_ok());
    let generated = format!("workspace-{}", Uuid::new_v4());
    assert!(WindowLabel::parse(&generated).is_ok());
    for invalid in [
        "",
        " workspace",
        "workspace/1",
        "workspace-00000000-0000-0000-0000-000000000000",
        "workspace-<script>",
        "workspace\n",
    ] {
        assert!(WindowLabel::parse(invalid).is_err());
    }
    assert!(WindowLabel::parse(&generated.to_uppercase()).is_err());
    assert!(
        directory
            .select(Some(&WindowLabel::parse(&generated).unwrap()))
            .is_err()
    );
}
#[test]
fn unpublished_or_unready_windows_cannot_route_and_default_never_guesses() {
    let directory = WindowDirectory::new(SessionRegistry::new());
    let a = directory.reserve(true).unwrap();
    ready(&directory, &a);
    assert!(directory.list().unwrap().is_empty());
    assert!(directory.select(Some(a.label())).is_err());
    directory.publish(&a).unwrap();
    let route = directory.select(None).unwrap();
    assert!(directory.current(&route).is_ok());
    let b = directory.reserve(false).unwrap();
    directory.publish(&b).unwrap();
    assert!(directory.select(None).is_err());
    assert!(directory.select(Some(b.label())).is_err());
    assert!(directory.current(&route).is_ok());
    ready(&directory, &b);
    assert!(directory.select(Some(b.label())).is_ok());
    assert!(directory.publish(&a).is_err());
}
#[test]
fn navigation_in_one_window_revokes_only_its_original_route() {
    let directory = WindowDirectory::new(SessionRegistry::new());
    let a = directory.reserve(true).unwrap();
    let b = directory.reserve(false).unwrap();
    for registration in [&a, &b] {
        directory.publish(registration).unwrap();
        ready(&directory, registration);
    }
    let old = directory.select(Some(a.label())).unwrap();
    let other = directory.select(Some(b.label())).unwrap();
    ready(&directory, &a);
    assert!(directory.current(&old).is_err());
    assert!(directory.current(&other).is_ok());
    assert!(
        directory
            .current(&directory.select(Some(a.label())).unwrap())
            .is_ok()
    );
}
#[test]
fn foreign_directory_registration_route_and_drain_are_rejected() {
    let a = WindowDirectory::new(SessionRegistry::new());
    let b = WindowDirectory::new(SessionRegistry::new());
    let registration = a.reserve(true).unwrap();
    assert!(b.publish(&registration).is_err());
    a.publish(&registration).unwrap();
    ready(&a, &registration);
    let route = a.select(None).unwrap();
    assert!(b.current(&route).is_err());
    let drain = a.begin_close(registration.label()).unwrap();
    assert!(b.complete_close(drain).is_err());
    // A wrong-directory attempt drops only its waiter; the original slot remains.
    let recovered = a.begin_close(registration.label()).unwrap();
    a.complete_close(recovered).unwrap();
}
#[test]
fn close_revokes_one_page_without_retargeting_old_primary_or_other_window() {
    let registry = SessionRegistry::new();
    let directory = WindowDirectory::new(registry.clone());
    let a = directory.reserve(true).unwrap();
    let b = directory.reserve(false).unwrap();
    for registration in [&a, &b] {
        directory.publish(registration).unwrap();
        ready(&directory, registration);
    }
    let old = directory.select(Some(a.label())).unwrap();
    let surviving = directory.select(Some(b.label())).unwrap();
    let drain = directory.begin_close(a.label()).unwrap();
    assert!(directory.current(&old).is_err());
    assert!(directory.current(&surviving).is_ok());
    assert!(registry.check_launch_page(&old.scope).is_err());
    assert!(registry.check_launch_page(&surviving.scope).is_ok());
    directory.complete_close(drain).unwrap();
    assert!(directory.reserve(true).is_err());
    assert!(directory.select(Some(a.label())).is_err());
    assert_eq!(directory.select(None).unwrap().label(), b.label());
}
#[test]
fn cancelled_native_creation_retains_capacity_until_explicit_drain() {
    let directory = WindowDirectory::new(SessionRegistry::new());
    let registrations: Vec<_> = (0..MAX_WINDOWS)
        .map(|_| directory.reserve(false).unwrap())
        .collect();
    let label = registrations[0].label().clone();
    drop(registrations);
    assert!(matches!(directory.reserve(false), Err(RegistryError::Busy)));
    let drain = directory.begin_close(&label).unwrap();
    assert!(directory.begin_close(&label).is_err());
    drop(drain);
    assert!(matches!(directory.reserve(false), Err(RegistryError::Busy)));
    directory
        .complete_close(directory.begin_close(&label).unwrap())
        .unwrap();
    assert!(directory.reserve(false).is_ok());
}
#[test]
fn old_registration_drop_cannot_release_replacement_capacity() {
    let directory = WindowDirectory::new(SessionRegistry::new());
    let old = directory.reserve(false).unwrap();
    let label = old.label().clone();
    directory
        .complete_close(directory.begin_close(&label).unwrap())
        .unwrap();
    let registrations: Vec<_> = (0..MAX_WINDOWS)
        .map(|_| directory.reserve(false).unwrap())
        .collect();
    drop(old);
    assert!(matches!(directory.reserve(false), Err(RegistryError::Busy)));
    assert!(directory.select(Some(&label)).is_err());
    drop(registrations);
}
#[test]
fn application_shutdown_blocks_routing_but_keeps_drain_slots() {
    let directory = WindowDirectory::new(SessionRegistry::new());
    let registration = directory.reserve(true).unwrap();
    directory.publish(&registration).unwrap();
    ready(&directory, &registration);
    let route = directory.select(None).unwrap();
    directory.shutdown().unwrap();
    assert!(directory.current(&route).is_err());
    assert!(directory.list().is_err());
    assert!(directory.reserve(false).is_err());
    directory
        .complete_close(directory.begin_close(registration.label()).unwrap())
        .unwrap();
    assert!(directory.0.state.lock().unwrap().entries.is_empty());
}

#[test]
fn failed_page_removal_still_revokes_native_owner_and_remaining_windows() {
    let registry = SessionRegistry::new();
    let directory = WindowDirectory::new(registry.clone());
    let a = directory.reserve(true).unwrap();
    let b = directory.reserve(false).unwrap();
    for registration in [&a, &b] {
        directory.publish(registration).unwrap();
        ready(&directory, registration);
    }
    let surviving_scope = directory
        .current(&directory.select(Some(b.label())).unwrap())
        .unwrap();
    directory.documents().remove(a.owner()).unwrap();
    // Primary is processed first. Failure must not skip closing its native owner or B.
    assert!(directory.shutdown().is_err());
    assert!(registry.activate_launch_page(a.owner()).is_err());
    assert!(registry.check_launch_page(&surviving_scope).is_err());
    assert!(directory.begin_close(a.label()).is_err());
    directory
        .complete_close(directory.begin_close(b.label()).unwrap())
        .unwrap();
    assert_eq!(directory.0.state.lock().unwrap().entries.len(), 1);
}
#[test]
#[allow(clippy::panic)]
fn dropping_a_poisoned_directory_revokes_pages_without_admitting_again() {
    let registry = SessionRegistry::new();
    let directory = WindowDirectory::new(registry.clone());
    let registration = directory.reserve(true).unwrap();
    directory.publish(&registration).unwrap();
    ready(&directory, &registration);
    let route = directory.select(None).unwrap();
    let scope = directory.current(&route).unwrap();
    let poisoned = directory.clone();
    let thread = std::thread::spawn(move || {
        let _guard = poisoned.0.state.lock().unwrap();
        panic!("simulated directory poison");
    });
    assert!(thread.join().is_err());
    assert!(directory.current(&route).is_err());
    assert!(directory.reserve(false).is_err());
    drop(directory);
    assert!(registry.check_launch_page(&scope).is_err());
}
