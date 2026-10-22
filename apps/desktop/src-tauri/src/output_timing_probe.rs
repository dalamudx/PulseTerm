type Result<T> = std::result::Result<T, &'static str>;

#[derive(serde::Deserialize, serde::Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct OutputTimings {
    schema_version: u8,
    overflow: bool,
    batches: Vec<OutputBatch>,
    frames: Vec<OutputFrame>,
}
#[derive(serde::Deserialize, serde::Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct OutputBatch {
    id: usize,
    offset: String,
    end: String,
    started_ms: f64,
    parsed_ms: f64,
    prepared_ms: f64,
    first_frame_ms: f64,
    last_frame_ms: f64,
    frame_count: u32,
    fenced_ms: f64,
    verified_ms: f64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    post_frame_task: Option<bool>,
}
#[derive(serde::Deserialize, serde::Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct OutputFrame {
    sequence: u32,
    offset: String,
    end: String,
    source_bytes: String,
    batch: usize,
    arrived_ms: f64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    acknowledge_started_ms: Option<f64>,
    acknowledged_ms: f64,
}
fn timing_coordinate(value: &str) -> Result<u64> {
    let number = value.parse::<u64>().map_err(|_| "workspace.denied")?;
    if number.to_string() != value {
        return Err("workspace.denied");
    }
    Ok(number)
}
fn timing_clock(value: f64) -> bool {
    value.is_finite() && (0.0..=3_600_000.0).contains(&value)
}
pub(super) fn output_timings(value: &str) -> Result<OutputTimings> {
    if value.len() > 128 * 1024 {
        return Err("workspace.denied");
    }
    let report: OutputTimings = serde_json::from_str(value).map_err(|_| "workspace.denied")?;
    if report.schema_version != 1
        || report.overflow
        || report.batches.is_empty()
        || report.batches.len() > 256
        || report.frames.is_empty()
        || report.frames.len() > 256
    {
        return Err("workspace.denied");
    }
    let mut cursors = Vec::with_capacity(report.batches.len());
    let mut previous_end = None;
    for (index, batch) in report.batches.iter().enumerate() {
        let start = timing_coordinate(&batch.offset)?;
        let end = timing_coordinate(&batch.end)?;
        let clocks = [
            batch.started_ms,
            batch.parsed_ms,
            batch.prepared_ms,
            batch.first_frame_ms,
            batch.last_frame_ms,
            batch.fenced_ms,
            batch.verified_ms,
        ];
        if batch.id != index + 1
            || end <= start
            || previous_end.is_some_and(|value| value != start)
            || !(2..=1024).contains(&batch.frame_count)
            || !clocks.into_iter().all(timing_clock)
            || clocks.windows(2).any(|pair| pair[0] > pair[1])
        {
            return Err("workspace.denied");
        }
        previous_end = Some(end);
        cursors.push(start);
    }
    let mut previous_sequence = None;
    let mut previous_source = 0;
    for frame in &report.frames {
        let index = frame.batch.checked_sub(1).ok_or("workspace.denied")?;
        let batch = report.batches.get(index).ok_or("workspace.denied")?;
        let start = timing_coordinate(&frame.offset)?;
        let end = timing_coordinate(&frame.end)?;
        let source = timing_coordinate(&frame.source_bytes)?;
        if previous_sequence.is_some_and(|value| value >= frame.sequence)
            || source < previous_source
            || start != cursors[index]
            || end <= start
            || end > timing_coordinate(&batch.end)?
            || !timing_clock(frame.arrived_ms)
            || !timing_clock(frame.acknowledged_ms)
            || frame.arrived_ms > batch.started_ms
            || frame.acknowledged_ms < batch.verified_ms
            || frame.acknowledge_started_ms.is_some_and(|started| {
                !timing_clock(started)
                    || started < batch.verified_ms
                    || started > frame.acknowledged_ms
            })
        {
            return Err("workspace.denied");
        }
        previous_sequence = Some(frame.sequence);
        previous_source = source;
        cursors[index] = end;
    }
    if report
        .batches
        .iter()
        .zip(cursors)
        .any(|(batch, cursor)| timing_coordinate(&batch.end) != Ok(cursor))
    {
        return Err("workspace.denied");
    }
    Ok(report)
}
/// Fixed probe only: source coordinates, sequence identity and native clocks; never bytes.
#[derive(Default)]
pub(super) struct NativeTimings {
    generation: u64,
    invalid: bool,
    frames: Vec<NativeFrame>,
}
#[derive(Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct NativeFrame {
    sequence: u32,
    offset: String,
    end: String,
    source_bytes: String,
    sent_ns: u64,
    send_returned_ns: Option<u64>,
    acknowledged_ns: Option<u64>,
}
impl NativeTimings {
    pub(super) fn new(generation: u64) -> Self {
        Self {
            generation,
            ..Self::default()
        }
    }
    pub(super) fn sending(&mut self, wire: &[u8], now: u64) -> Option<u32> {
        let decoded = pulse_ipc::decode(wire).ok().and_then(|frame| {
            let fragment = pulse_ipc::attach_v2::decode_fragment(frame.payload).ok()?;
            Some((frame.sequence, fragment))
        });
        let Some((sequence, fragment)) = decoded else {
            self.invalid = true;
            return None;
        };
        if fragment.generation != self.generation {
            self.invalid = true;
            return None;
        }
        if fragment.part != pulse_ipc::attach::Part::Data {
            return None;
        }
        if self.invalid {
            return None;
        }
        let end = fragment.offset.checked_add(fragment.body.len() as u64);
        if self.frames.len() >= 256
            || now == 0
            || end.is_none()
            || self
                .frames
                .last()
                .is_some_and(|last| last.sequence >= sequence || last.sent_ns > now)
        {
            self.invalid = true;
            return None;
        }
        self.frames.push(NativeFrame {
            sequence,
            offset: fragment.offset.to_string(),
            end: end?.to_string(),
            source_bytes: fragment.source_bytes.to_string(),
            sent_ns: now,
            send_returned_ns: None,
            acknowledged_ns: None,
        });
        Some(sequence)
    }
    pub(super) fn sent(&mut self, sequence: Option<u32>, now: u64, success: bool) {
        let Some(sequence) = sequence else {
            return;
        };
        let Some(frame) = self
            .frames
            .iter_mut()
            .find(|frame| frame.sequence == sequence)
        else {
            self.invalid = true;
            return;
        };
        if !success || now < frame.sent_ns || frame.send_returned_ns.is_some() {
            self.invalid = true;
            return;
        }
        frame.send_returned_ns = Some(now);
    }
    pub(super) fn acknowledged(&mut self, sequence: u32, now: u64) {
        let Some(frame) = self
            .frames
            .iter_mut()
            .find(|frame| frame.sequence == sequence)
        else {
            return;
        };
        if now < frame.sent_ns || frame.acknowledged_ns.is_some() {
            self.invalid = true;
            return;
        }
        frame.acknowledged_ns = Some(now);
    }
    pub(super) fn snapshot(&self, browser: &OutputTimings) -> Result<serde_json::Value> {
        if self.invalid || self.frames.is_empty() || self.frames.len() != browser.frames.len() {
            return Err("workspace.denied");
        }
        for (native, rendered) in self.frames.iter().zip(&browser.frames) {
            if native.sequence != rendered.sequence
                || native.offset != rendered.offset
                || native.end != rendered.end
                || native.source_bytes != rendered.source_bytes
                || native.send_returned_ns.is_none()
                || native.acknowledged_ns.is_none()
            {
                return Err("workspace.denied");
            }
        }
        Ok(
            serde_json::json!({ "schemaVersion": 1, "clockDomain": "Linux CLOCK_MONOTONIC",
            "generation": self.generation.to_string(), "frames": self.frames }),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn optional_ack_start_roundtrips_and_invalid_order_rejects() -> Result<()> {
        let mut report = fixture();
        report["frames"][0]["acknowledgeStartedMs"] = serde_json::json!(16.5);
        let parsed = output_timings(&report.to_string())?;
        assert_eq!(serde_json::to_value(parsed).map_err(|_| "fixture")?, report);
        for value in [-1.0, 15.0, 18.0] {
            report["frames"][0]["acknowledgeStartedMs"] = serde_json::json!(value);
            assert!(output_timings(&report.to_string()).is_err());
        }
        Ok(())
    }
    #[test]
    fn optional_render_mode_is_a_strict_boolean_and_roundtrips() -> Result<()> {
        for mode in [true, false] {
            let mut report = fixture();
            report["batches"][0]["postFrameTask"] = serde_json::json!(mode);
            assert_eq!(
                serde_json::to_value(output_timings(&report.to_string())?)
                    .map_err(|_| "fixture")?,
                report
            );
        }
        let mut report = fixture();
        report["batches"][0]["postFrameTask"] = serde_json::json!("true");
        assert!(output_timings(&report.to_string()).is_err());
        Ok(())
    }
    fn fixture() -> serde_json::Value {
        serde_json::json!({"schemaVersion":1,"overflow":false,"batches":[{
            "id":1,"offset":"1152921504606846983","end":"1152921504606846987",
            "startedMs":10.0,"parsedMs":11.0,"preparedMs":12.0,"firstFrameMs":13.0,
            "lastFrameMs":14.0,"frameCount":2,"fencedMs":15.0,"verifiedMs":16.0}],
            "frames":[{"sequence":8,"offset":"1152921504606846983","end":"1152921504606846987",
            "sourceBytes":"100","batch":1,"arrivedMs":5.0,"acknowledgedMs":17.0}]})
    }
    #[test]
    fn exact_metadata_preserves_both_domains_and_roundtrips_without_text() -> Result<()> {
        let value = fixture();
        let parsed = output_timings(&value.to_string())?;
        let output = serde_json::to_value(parsed).map_err(|_| "fixture")?;
        assert_eq!(output, value);
        Ok(())
    }
    #[test]
    fn unknown_content_fields_overflow_future_schema_and_oversize_reject() {
        for (field, value) in [
            ("terminalText", serde_json::json!("untrusted")),
            ("schemaVersion", serde_json::json!(2)),
            ("overflow", serde_json::json!(true)),
        ] {
            let mut report = fixture();
            report[field] = value;
            assert!(output_timings(&report.to_string()).is_err());
        }
        assert!(output_timings(&" ".repeat(128 * 1024 + 1)).is_err());
        let mut empty = fixture();
        empty["frames"] = serde_json::json!([]);
        assert!(output_timings(&empty.to_string()).is_err());
        let mut oversized = fixture();
        oversized["frames"] = serde_json::json!(vec![oversized["frames"][0].clone(); 257]);
        assert!(output_timings(&oversized.to_string()).is_err());
    }
    #[test]
    fn reversed_negative_long_clocks_and_unfinished_animation_reject() {
        for (field, value) in [
            ("parsedMs", -1.0),
            ("firstFrameMs", 9.0),
            ("verifiedMs", 3_600_001.0),
            ("fencedMs", 13.0),
        ] {
            let mut report = fixture();
            report["batches"][0][field] = serde_json::json!(value);
            assert!(output_timings(&report.to_string()).is_err());
        }
        let mut report = fixture();
        report["batches"][0]["frameCount"] = serde_json::json!(1);
        assert!(output_timings(&report.to_string()).is_err());
        assert!(output_timings(&fixture().to_string().replace("11.0", "NaN")).is_err());
    }
    #[test]
    fn frame_identity_geometry_coverage_and_clock_order_reject() {
        for (field, value) in [
            ("sequence", serde_json::json!(-1)),
            ("batch", serde_json::json!(2)),
            ("offset", serde_json::json!("1152921504606846984")),
            ("sourceBytes", serde_json::json!("01")),
            ("arrivedMs", serde_json::json!(11)),
            ("acknowledgedMs", serde_json::json!(15)),
        ] {
            let mut report = fixture();
            report["frames"][0][field] = value;
            assert!(output_timings(&report.to_string()).is_err());
        }
        let mut partial = fixture();
        partial["frames"][0]["end"] = serde_json::json!("1152921504606846985");
        assert!(output_timings(&partial.to_string()).is_err());
    }
    #[test]
    fn adjacent_frames_may_share_one_batch_but_cannot_duplicate_or_overlap() {
        let mut report = fixture();
        let mut second = report["frames"][0].clone();
        report["frames"][0]["end"] = serde_json::json!("1152921504606846985");
        second["offset"] = serde_json::json!("1152921504606846985");
        second["sequence"] = serde_json::json!(9);
        if let Some(frames) = report["frames"].as_array_mut() {
            frames.push(second);
        }
        assert!(output_timings(&report.to_string()).is_ok());
        report["frames"][1]["sequence"] = serde_json::json!(8);
        assert!(output_timings(&report.to_string()).is_err());
    }
    fn native_wire(
        sequence: u32,
        generation: u64,
        part: pulse_ipc::attach::Part,
    ) -> Result<Vec<u8>> {
        let payload = pulse_ipc::attach_v2::Fragment {
            generation,
            part,
            offset: (1 << 60) + 7,
            source_bytes: 100,
            body: &[97; 4],
        }
        .encode()
        .map_err(|_| "fixture")?;
        pulse_ipc::Frame {
            kind: pulse_ipc::FrameKind::Attachment,
            sequence,
            payload: &payload,
        }
        .encode()
        .map_err(|_| "fixture")
    }
    #[test]
    fn native_clock_report_matches_exact_browser_sequence_and_both_coordinates() -> Result<()> {
        let mut native = NativeTimings::new(9);
        let sequence = native.sending(&native_wire(8, 9, pulse_ipc::attach::Part::Data)?, 1000);
        native.sent(sequence, 1001, true);
        native.acknowledged(8, 2000);
        let browser = output_timings(&fixture().to_string())?;
        let report = native.snapshot(&browser)?;
        assert_eq!(report["frames"][0]["sentNs"], 1000);
        assert_eq!(report["frames"][0]["offset"], "1152921504606846983");
        assert_eq!(report["frames"][0]["sourceBytes"], "100");
        assert!(report["frames"][0].get("body").is_none());
        for (key, value) in [
            ("sequence", serde_json::json!(9)),
            ("sourceBytes", serde_json::json!("101")),
        ] {
            let mut wrong = fixture();
            wrong["frames"][0][key] = value;
            assert!(
                native
                    .snapshot(&output_timings(&wrong.to_string())?)
                    .is_err()
            );
        }
        Ok(())
    }
    #[test]
    fn native_incomplete_failed_duplicate_or_backwards_events_cannot_export() -> Result<()> {
        let browser = output_timings(&fixture().to_string())?;
        let wire = native_wire(8, 9, pulse_ipc::attach::Part::Data)?;
        for kind in [
            "missing-send",
            "missing-ack",
            "failed-send",
            "duplicate-ack",
            "backwards",
        ] {
            let mut native = NativeTimings::new(9);
            let sequence = native.sending(&wire, 1000);
            if kind != "missing-send" {
                native.sent(sequence, 1001, kind != "failed-send");
            }
            if kind != "missing-ack" {
                native.acknowledged(8, if kind == "backwards" { 999 } else { 2000 });
            }
            if kind == "duplicate-ack" {
                native.acknowledged(8, 2001);
            }
            assert!(native.snapshot(&browser).is_err(), "{kind}");
        }
        Ok(())
    }
    #[test]
    fn native_unknown_wire_foreign_generation_or_reordered_send_reject() -> Result<()> {
        let browser = output_timings(&fixture().to_string())?;
        for wire in [vec![0], native_wire(8, 10, pulse_ipc::attach::Part::Data)?] {
            let mut native = NativeTimings::new(9);
            assert_eq!(native.sending(&wire, 1000), None);
            assert!(native.snapshot(&browser).is_err());
        }
        let mut native = NativeTimings::new(9);
        let wire = native_wire(8, 9, pulse_ipc::attach::Part::Data)?;
        let sequence = native.sending(&wire, 1000);
        native.sent(sequence, 1001, true);
        native.acknowledged(8, 2000);
        assert_eq!(native.sending(&wire, 3000), None);
        assert!(native.snapshot(&browser).is_err());
        Ok(())
    }
    #[test]
    fn native_frame_budget_is_bounded_and_control_ack_does_not_create_data() -> Result<()> {
        let mut native = NativeTimings::new(9);
        native.acknowledged(1, 500);
        for sequence in 1..=256 {
            let seq = native.sending(
                &native_wire(sequence, 9, pulse_ipc::attach::Part::Data)?,
                1000 + u64::from(sequence),
            );
            native.sent(seq, 2000 + u64::from(sequence), true);
            native.acknowledged(sequence, 3000 + u64::from(sequence));
        }
        assert_eq!(native.frames.len(), 256);
        assert_eq!(
            native.sending(&native_wire(257, 9, pulse_ipc::attach::Part::Data)?, 4000),
            None
        );
        assert!(native.invalid);
        assert_eq!(native.frames.len(), 256);
        Ok(())
    }
}
