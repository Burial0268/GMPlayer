use std::io::Write;
use std::sync::mpsc;

use super::*;
use protocol::{AuthReply, SetClientNameReply, SubscriptionEvent, SubscriptionEventType};

fn handshake(server: &mut BufReader<UnixStream>) {
    let version = protocol::MAX_VERSION;
    let (sequence, command) = protocol::read_command_message(server, version).unwrap();
    assert_eq!(sequence, 0);
    assert!(matches!(command, Command::Auth(_)));
    protocol::write_reply_message(
        server.get_mut(),
        sequence,
        &AuthReply {
            version,
            ..AuthReply::default()
        },
        version,
    )
    .unwrap();
    let (sequence, command) = protocol::read_command_message(server, version).unwrap();
    assert_eq!(sequence, 1);
    assert!(matches!(command, Command::SetClientName(_)));
    protocol::write_reply_message(
        server.get_mut(),
        sequence,
        &SetClientNameReply { client_id: 1 },
        version,
    )
    .unwrap();
    let (sequence, command) = protocol::read_command_message(server, version).unwrap();
    assert_eq!(sequence, 2);
    assert!(
        matches!(command, Command::Subscribe(mask) if mask == SubscriptionMask::SINK | SubscriptionMask::SERVER)
    );
    protocol::write_ack_message(server.get_mut(), sequence).unwrap();
}

#[test]
fn subscription_handles_framed_events_and_ignores_unrelated_facilities() {
    let (client, server) = UnixStream::pair().unwrap();
    let pending = Arc::new(PendingChanges::default());
    let worker_pending = Arc::clone(&pending);
    let worker = thread::spawn(move || subscribe(client, &worker_pending, || {}));
    let mut server = BufReader::new(server);
    server
        .get_ref()
        .set_read_timeout(Some(Duration::from_secs(3)))
        .unwrap();
    handshake(&mut server);
    let mut events = Vec::new();
    for facility in [
        SubscriptionEventFacility::Sink,
        SubscriptionEventFacility::Server,
        SubscriptionEventFacility::Source,
    ] {
        protocol::write_command_message(
            &mut events,
            u32::MAX,
            &Command::SubscribeEvent(SubscriptionEvent {
                event_facility: facility,
                event_type: SubscriptionEventType::Changed,
                index: Some(7),
            }),
            protocol::MAX_VERSION,
        )
        .unwrap();
    }
    for fragment in events.chunks(3) {
        server.get_mut().write_all(fragment).unwrap();
    }
    server.get_ref().shutdown(Shutdown::Write).unwrap();
    assert!(
        worker.join().unwrap().is_err(),
        "EOF must end the subscription"
    );
    assert_eq!(pending.take().0, DEFAULT_CHANGED | DEVICES_CHANGED);
    assert_eq!(pending.take().0, 0);

    record_event(&pending, SubscriptionEventFacility::Source);
    record_event(&pending, SubscriptionEventFacility::SinkInput);
    assert_eq!(pending.take().0, 0);
}

#[test]
fn dropping_the_watcher_wakes_an_idle_subscription_and_closes_its_socket() {
    let (client, server) = UnixStream::pair().unwrap();
    let control = Arc::new(Control {
        socket: Mutex::new(Some(client.try_clone().unwrap())),
        ..Control::default()
    });
    let pending = Arc::new(PendingChanges::default());
    let (ready_tx, ready_rx) = mpsc::sync_channel(1);
    let worker = thread::spawn(move || {
        let _ = subscribe(client, &pending, || ready_tx.send(()).unwrap());
    });
    let backend = Backend {
        control,
        thread: Some(worker),
    };
    let mut server = BufReader::new(server);
    server
        .get_ref()
        .set_read_timeout(Some(Duration::from_secs(3)))
        .unwrap();
    handshake(&mut server);
    ready_rx.recv_timeout(Duration::from_secs(3)).unwrap();
    let (done_tx, done_rx) = mpsc::sync_channel(1);
    thread::spawn(move || {
        drop(backend);
        done_tx.send(()).unwrap();
    });
    done_rx
        .recv_timeout(Duration::from_secs(3))
        .expect("idle reader was not woken");
    use std::io::Read;
    assert_eq!(server.read(&mut [0]).unwrap(), 0);
}

#[test]
fn an_unanswered_handshake_is_bounded() {
    let (client, server) = UnixStream::pair().unwrap();
    let (done_tx, done_rx) = mpsc::sync_channel(1);
    thread::spawn(move || {
        let result = subscribe(client, &PendingChanges::default(), || {
            panic!("not authenticated")
        });
        done_tx.send(result.is_err()).unwrap();
    });
    let mut server = BufReader::new(server);
    server
        .get_ref()
        .set_read_timeout(Some(Duration::from_secs(3)))
        .unwrap();
    assert!(matches!(
        protocol::read_command_message(&mut server, protocol::MAX_VERSION)
            .unwrap()
            .1,
        Command::Auth(_)
    ));
    assert!(done_rx
        .recv_timeout(Duration::from_secs(4))
        .expect("handshake did not time out"));
}

#[test]
#[ignore = "run with scripts/test-audio-device-watch.sh (isolated PulseAudio server)"]
fn live_default_switch_is_notified_and_resolves_to_the_new_output() {
    use crate::output::{self, device_watcher::DeviceWatcher, OutputDeviceSelector};
    use std::process::Command as ProcessCommand;
    use std::time::Instant;

    assert_eq!(
        std::env::var("GMPLAYER_DEVICE_WATCH_TEST").as_deref(),
        Ok("1")
    );
    let pactl = |arguments: &[&str]| {
        let result = ProcessCommand::new("pactl")
            .args(arguments)
            .output()
            .unwrap();
        assert!(
            result.status.success(),
            "{}",
            String::from_utf8_lossy(&result.stderr)
        );
    };
    let wait_for_change = |watcher: &DeviceWatcher, expected: u8| {
        let start = Instant::now();
        loop {
            if watcher.take_changes().0 & expected == expected {
                return;
            }
            assert!(
                start.elapsed() < RECONNECT_DELAY + Duration::from_secs(3),
                "device notification missing"
            );
            thread::sleep(Duration::from_millis(10));
        }
    };

    pactl(&["set-default-sink", "gmplayer_watch_a"]);
    let watcher = DeviceWatcher::new();
    wait_for_change(&watcher, DEFAULT_CHANGED | DEVICES_CHANGED);
    let selector = OutputDeviceSelector::Default;
    let before = output::selected_output_device_key(&selector).unwrap();
    let first = output::open_output(selector.clone(), None).unwrap();
    assert_eq!(first.device(), &before);

    pactl(&["set-default-sink", "gmplayer_watch_b"]);
    wait_for_change(&watcher, DEFAULT_CHANGED);
    let after = output::refreshed_output_device_key(&selector).unwrap();
    assert_ne!(before, after, "the probe must resolve the new sink");
    let second = output::open_output(selector, None).unwrap();
    assert_eq!(second.device(), &after);
    assert!(!second.has_failed());

    pactl(&[
        "load-module",
        "module-null-sink",
        "sink_name=gmplayer_watch_c",
    ]);
    wait_for_change(&watcher, DEVICES_CHANGED);
    assert_eq!(
        output::selected_output_device_key(&OutputDeviceSelector::Default).unwrap(),
        after
    );
    // Break the listener's connection without restarting the player or server.
    watcher.take_changes();
    watcher
        ._backend
        .as_ref()
        .unwrap()
        .control
        .socket
        .lock()
        .unwrap()
        .as_ref()
        .unwrap()
        .shutdown(Shutdown::Both)
        .unwrap();
    wait_for_change(&watcher, DEFAULT_CHANGED | DEVICES_CHANGED);
    pactl(&["set-default-sink", "gmplayer_watch_a"]);
    wait_for_change(&watcher, DEFAULT_CHANGED);
    assert_eq!(
        output::selected_output_device_key(&OutputDeviceSelector::Default).unwrap(),
        before
    );

    drop(second);
    drop(first);
    drop(watcher);
}

#[test]
fn a_mismatched_handshake_reply_is_rejected() {
    let (client, server) = UnixStream::pair().unwrap();
    let worker = thread::spawn(move || {
        subscribe(client, &PendingChanges::default(), || {
            panic!("not authenticated")
        })
    });
    let mut server = BufReader::new(server);
    protocol::read_command_message(&mut server, protocol::MAX_VERSION).unwrap();
    protocol::write_reply_message(
        server.get_mut(),
        99,
        &AuthReply {
            version: protocol::MAX_VERSION,
            ..AuthReply::default()
        },
        protocol::MAX_VERSION,
    )
    .unwrap();
    assert!(worker
        .join()
        .unwrap()
        .unwrap_err()
        .to_string()
        .contains("sequence"));
}
