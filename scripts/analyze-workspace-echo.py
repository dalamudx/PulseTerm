#!/usr/bin/env python3
"""Correlate existing Linux X11/PTY clocks without attributing unmeasured IPC stages."""
import argparse
import hashlib
import json
import math
from pathlib import Path


def percentiles(values):
    ordered = sorted(values)
    if not ordered:
        raise ValueError("Empty timing population")
    return {name: ordered[math.ceil(len(ordered) * rank) - 1] for name, rank in
            (("p50Ms", .5), ("p95Ms", .95), ("p99Ms", .99), ("maxMs", 1))}


def analyze_pacing(timing):
    if 'pacingMode' not in timing and 'requestedPauseNs' not in timing:
        return {'available': False, 'reason': 'Historical report has no explicit pacing metadata'}
    mode, pauses = timing.get('pacingMode'), timing.get('requestedPauseNs')
    if mode not in ('fixed', 'jitter') or not isinstance(pauses, list) or len(pauses) != 100:
        raise ValueError('Incomplete pacing metadata')
    if any(type(value) is not int or not 5_000_000 <= value <= 35_000_000 for value in pauses):
        raise ValueError('Invalid pacing delays')
    if mode == 'fixed' and any(value != 20_000_000 for value in pauses):
        raise ValueError('Fixed pacing must retain 20ms')
    if mode == 'jitter' and (len(set(pauses)) < 2 or any(sum(pauses[i:i + 2]) != 40_000_000 for i in range(0, 100, 2))):
        raise ValueError('Jitter pacing must use complementary 20ms-mean pairs')
    starts, observed = timing['startNs'], timing['observedNs']
    return {'available': True, 'mode': mode, 'requestedPauseMeanMs': sum(pauses) / len(pauses) / 1_000_000,
            'requestedPauseMinMs': min(pauses) / 1_000_000, 'requestedPauseMaxMs': max(pauses) / 1_000_000,
            'actualPostObservationGapMs': percentiles([(start - end) / 1_000_000 for start, end in zip(starts[1:], observed[:-1])]),
            'actualInterInputMs': percentiles([(b - a) / 1_000_000 for a, b in zip(starts, starts[1:])]),
            'limitations': ['Requested inter-sample pauses are not input-to-glyph durations and are never subtracted',
                            'Observed gaps also contain sleep scheduling, initial pixel read and keycode lookup',
                            'Deterministic pacing variation is a phase-sensitivity probe, not randomized user-input acceptance']}


def analyze(timing, native):
    samples, starts, observations = (timing.get(name) for name in ('samplesMs', 'startNs', 'observedNs'))
    if any(not isinstance(values, list) or len(values) != 100 for values in (samples, starts, observations, native)):
        raise ValueError('Exactly 100 correlated samples are required; no truncation or dropped tail samples')
    stages = {'inputToNative': [], 'nativeHandling': [], 'nativeWriteToGlyph': [], 'total': []}
    rows = []
    for index, (sample, start, observed, event) in enumerate(zip(samples, starts, observations, native, strict=True)):
        if not isinstance(event, dict):
            raise ValueError('Invalid native timing event')
        received, written = event.get('receivedNs'), event.get('writtenNs')
        if any(type(value) is not int or value < 0 for value in (start, received, written, observed)):
            raise ValueError('Native monotonic timestamps must be nonnegative integers')
        if not start <= received <= written <= observed or observed == start:
            raise ValueError('Uncorrelated native write and observed glyph')
        if index and start <= observations[index - 1]:
            raise ValueError('Serial sample windows overlap or run backwards')
        if type(sample) not in (int, float) or not math.isfinite(sample) or sample <= 0:
            raise ValueError('Invalid elapsed sample')
        measured = (observed - start) / 1_000_000
        if not math.isclose(sample, measured, rel_tol=0, abs_tol=0.000001):
            raise ValueError('Elapsed sample differs from raw monotonic timestamps')
        parts = {'inputToNative': (received - start) / 1_000_000,
                 'nativeHandling': (written - received) / 1_000_000,
                 'nativeWriteToGlyph': (observed - written) / 1_000_000,
                 'total': measured}
        for name, value in parts.items():
            stages[name].append(value)
        rows.append({'index': index, **parts})
    return {
        'schemaVersion': 1, 'analysisComplete': True, 'sampleCount': 100,
        'warmupsExcluded': 0, 'slowSamplesExcluded': 0,
        'clockDomain': 'shared Linux CLOCK_MONOTONIC: XTest/pixel helper and native PTY echo fixture',
        'percentileMethod': 'nearest rank, computed separately from the same complete sample set',
        'stages': {name: percentiles(values) for name, values in stages.items()},
        'localEchoTargetP95Ms': 16, 'targetMet': percentiles(stages['total'])['p95Ms'] < 16,
        'samples': rows,
        "pacing": analyze_pacing(timing),
        'limitations': [
            'Linux X11 software-rendered measurement; not hardware, Windows or macOS acceptance',
            'nativeWriteToGlyph includes PTY reader, backend parsing/queues, IPC, xterm and presentation; these parts are not separately measured',
            'Stage percentiles must not be added; only individual sample durations form a total',
            'No claim about physical key-to-photon, flood-load echo, SSH or recovery latency',
        ],
    }


def analyze_transport(timing, native, browser, transport):
    """Join native submission to pixels in CLOCK_MONOTONIC, never browser absolute time."""
    analyze(timing, native)
    analyze_frontend(browser)
    frames = transport.get('frames')
    generation = transport.get('generation')
    if (transport.get('schemaVersion') != 1 or transport.get('clockDomain') != 'Linux CLOCK_MONOTONIC'
            or not isinstance(generation, str) or not generation.isascii() or not generation.isdecimal()
            or not 0 < int(generation) < 2**64 or str(int(generation)) != generation
            or not isinstance(frames, list) or len(frames) != len(browser['frames'])):
        raise ValueError('Invalid native transport identity, clock or population')
    previous = 0
    for frame, rendered in zip(frames, browser['frames'], strict=True):
        if any(frame.get(key) != rendered[key] for key in ('sequence', 'offset', 'end', 'sourceBytes')):
            raise ValueError('Native and browser frame identities differ')
        sent, returned, acknowledged = (frame.get(key) for key in ('sentNs', 'sendReturnedNs', 'acknowledgedNs'))
        if (any(type(value) is not int or value <= 0 or value >= 2**64 for value in (sent, returned, acknowledged))
                or sent < previous or returned < sent or acknowledged < sent):
            raise ValueError('Incomplete or backwards native transport timestamps')
        previous = sent
    stages = {name: [] for name in ('nativeWriteStartToChannel', 'channelSubmissionCall',
                                    'channelSubmitToGlyph', 'channelSubmitToNativeAck')}
    samples, used = [], set()
    for index, (echo, observed) in enumerate(zip(native, timing['observedNs'], strict=True)):
        started = echo.get('writeStartedNs')
        if type(started) is not int or not echo['receivedNs'] <= started <= echo['writtenNs'] <= observed:
            raise ValueError('Missing or uncorrelated native write-start clock')
        candidates = [frame for frame in frames if started <= frame['sentNs'] <= observed]
        if len(candidates) != 1 or candidates[0]['sequence'] in used:
            raise ValueError('A physical sample must match exactly one unique native data frame')
        frame = candidates[0]
        used.add(frame['sequence'])
        parts = {'nativeWriteStartToChannel': (frame['sentNs'] - started) / 1_000_000,
                 'channelSubmissionCall': (frame['sendReturnedNs'] - frame['sentNs']) / 1_000_000,
                 'channelSubmitToGlyph': (observed - frame['sentNs']) / 1_000_000,
                 'channelSubmitToNativeAck': (frame['acknowledgedNs'] - frame['sentNs']) / 1_000_000}
        for key, value in parts.items():
            stages[key].append(value)
        rendered = next(row for row in browser["frames"] if row["sequence"] == frame["sequence"])
        mode = browser["batches"][rendered["batch"] - 1].get("postFrameTask")
        samples.append({'index': index, 'sequence': frame['sequence'], 'offset': frame['offset'],
                        'end': frame['end'], 'sourceBytes': frame['sourceBytes'], 'postFrameTask': mode, **parts})
    return {'clockDomain': 'shared Linux CLOCK_MONOTONIC: PTY fixture, native Channel sender and pixel helper',
            'sampleCount': len(samples), 'nativeFrameCount': len(frames), 'browserFrameIdentityMatched': True,
            'stages': {name: percentiles(values) for name, values in stages.items()}, 'samples': samples,
            'deliveryBounds': bound_delivery(samples, browser),
            'limitations': ['Channel submission is not browser receipt or compositor acknowledgement',
                            'nativeWriteStartToChannel includes write syscall, PTY reader, backend and queues',
                            'channelSubmitToGlyph includes IPC delivery, browser processing and presentation',
                            'Native ACK time is observed after page/ticket admission and before binding ACK processing',
                            'No isolated one-way IPC claim or browser/native absolute clock subtraction',
                            'Single-frame correlation is required; missing, split or ambiguous samples fail without dropping tails']}


def bound_delivery(samples, browser):
    """Causal interval bounds from an existing ACK loop, with explicit clock assumptions."""
    if not all('acknowledgeStartedMs' in frame for frame in browser['frames']):
        return {'available': False, 'reason': 'Historical report has no ACK-start times; no alignment inferred'}
    # Assumed bounds, not a WebView clock calibration or a measured precision guarantee.
    read_error_ms, relative_rate_ppm = 2.0, 100.0
    rendered_by_sequence = {frame['sequence']: frame for frame in browser['frames']}
    rows = []
    for sample in samples:
        rendered = rendered_by_sequence[sample['sequence']]
        batch = browser['batches'][rendered['batch'] - 1]
        browser_until_ack = rendered['acknowledgeStartedMs'] - rendered['arrivedMs']
        browser_until_verified = batch['verifiedMs'] - rendered['arrivedMs']
        verified_until_ack = rendered['acknowledgeStartedMs'] - batch['verifiedMs']
        native_loop = sample['channelSubmitToNativeAck']
        elapsed_error = 2 * read_error_ms + max(native_loop, browser_until_ack) * relative_rate_ppm / 1_000_000
        residual = native_loop - browser_until_ack
        if residual + elapsed_error < 0:
            raise ValueError('ACK loop contradicts browser timings under the declared clock error assumptions')
        upper_delivery = max(0, residual + elapsed_error)
        lower_post_verify = sample['channelSubmitToGlyph'] - native_loop + verified_until_ack - elapsed_error
        upper_post_verify = sample['channelSubmitToGlyph'] - browser_until_verified + elapsed_error
        if lower_post_verify > upper_post_verify:
            raise ValueError('Inconsistent post-verification interval')
        rows.append({'index': sample['index'], 'sequence': sample['sequence'],
                     'elapsedErrorBudgetMs': elapsed_error,
                     'channelDeliveryLowerMs': 0.0, 'channelDeliveryUpperMs': upper_delivery,
                     'verifiedToGlyphLowerMs': lower_post_verify, 'verifiedToGlyphUpperMs': upper_post_verify})
    return {'available': True, 'sampleCount': len(rows),
            'method': 'Causal duration inequalities in matched submit/receive/ACK-start/native-ACK loops; no absolute clock subtraction',
            'assumptions': {'browserSingleReadErrorAtMostMs': read_error_ms, 'relativeClockRateErrorAtMostPpm': relative_rate_ppm,
                            'independentlyCalibrated': False},
            'stages': {name: percentiles([row[name] for row in rows]) for name in
                       ('channelDeliveryUpperMs', 'verifiedToGlyphLowerMs', 'verifiedToGlyphUpperMs')},
            'postVerificationDelayEstablishedSamplesUnderAssumptions': sum(row['verifiedToGlyphLowerMs'] > 0 for row in rows),
            'samples': rows,
            'limitations': ['Bounds are conditional on the declared clock-error assumptions; not calibrated one-way IPC measurements',
                            'ACK return-path and native admission costs widen the forward-delivery upper bound',
                            'Signed post-verification intervals can be negative because a pixel may appear before verification',
                            'Pixel observation includes X11 helper polling and does not certify physical compositor presentation',
                            'Individual inequalities, never subtraction of stage percentiles']}


def analyze_frontend(report):
    """Browser-local spans end at ACK, not at physical pixel presentation."""
    batches, frames = report.get('batches'), report.get('frames')
    if (report.get('schemaVersion') != 1 or report.get('overflow') is not False
            or not isinstance(batches, list) or not 1 <= len(batches) <= 256
            or not isinstance(frames, list) or not 1 <= len(frames) <= 256):
        raise ValueError('Incomplete or oversized browser timing report')

    ack_starts = ["acknowledgeStartedMs" in frame for frame in frames]
    if any(ack_starts) and not all(ack_starts):
        raise ValueError("Partial browser ACK-start metadata")
    def coordinate(value):
        if not isinstance(value, str) or not value.isascii() or not value.isdecimal() or len(value) > 20:
            raise ValueError('Invalid browser stream coordinate')
        parsed = int(value)
        if str(parsed) != value or parsed >= 2**64:
            raise ValueError('Noncanonical or overflowing browser coordinate')
        return parsed

    def clock(value):
        return type(value) in (int, float) and math.isfinite(value) and 0 <= value <= 3_600_000

    stages = {name: [] for name in ('parser', 'prepareDraw', 'firstAnimationWait', 'remainingFence', 'verifyDraw',
                                    'queueToParser', 'verifiedToAckCompletion', 'entryToAckCompletion')}
    cursors, ends = [], []
    previous_end = None
    for index, batch in enumerate(batches):
        if "postFrameTask" in batch and type(batch["postFrameTask"]) is not bool:
            raise ValueError("Invalid render scheduling mode")
        start, end = coordinate(batch['offset']), coordinate(batch['end'])
        clocks = [batch[key] for key in ('startedMs', 'parsedMs', 'preparedMs', 'firstFrameMs',
                                         'lastFrameMs', 'fencedMs', 'verifiedMs')]
        if (type(batch['id']) is not int or batch['id'] != index + 1 or end <= start
                or previous_end is not None and previous_end != start
                or type(batch['frameCount']) is not int or not 2 <= batch['frameCount'] <= 1024
                or not all(clock(value) for value in clocks)
                or any(a > b for a, b in zip(clocks, clocks[1:]))):
            raise ValueError('Invalid browser batch clocks or boundaries')
        previous_end = end
        cursors.append(start)
        ends.append(end)
        for name, value in zip(('parser', 'prepareDraw', 'firstAnimationWait', 'remainingFence', 'verifyDraw'),
                               (clocks[1] - clocks[0], clocks[2] - clocks[1], clocks[3] - clocks[2],
                                clocks[5] - clocks[3], clocks[6] - clocks[5]), strict=True):
            stages[name].append(value)
    previous_sequence = -1
    previous_source = 0
    for frame in frames:
        batch_id = frame['batch']
        if type(batch_id) is not int or not 1 <= batch_id <= len(batches):
            raise ValueError('Invalid browser batch association')
        batch = batches[batch_id - 1]
        start, end, source = (coordinate(frame[key]) for key in ('offset', 'end', 'sourceBytes'))
        arrived, acknowledged = frame['arrivedMs'], frame['acknowledgedMs']
        sequence = frame['sequence']
        if (type(sequence) is not int or not previous_sequence < sequence < 2**32
                or source < previous_source or start != cursors[batch_id - 1] or not start < end <= ends[batch_id - 1]
                or not clock(arrived) or not clock(acknowledged)
                or arrived > batch['startedMs'] or acknowledged < batch['verifiedMs']):
            raise ValueError('Invalid browser frame coverage, identity or clocks')
        if all(ack_starts):
            started = frame["acknowledgeStartedMs"]
            if not clock(started) or not batch["verifiedMs"] <= started <= acknowledged:
                raise ValueError("Invalid browser ACK-start clock")
        cursors[batch_id - 1] = end
        previous_sequence, previous_source = sequence, source
        stages['queueToParser'].append(batch['startedMs'] - arrived)
        stages['verifiedToAckCompletion'].append(acknowledged - batch['verifiedMs'])
        stages['entryToAckCompletion'].append(acknowledged - arrived)
    if cursors != ends:
        raise ValueError('Incomplete browser frame coverage')
    return {'clockDomain': 'browser performance.now only; not aligned to native CLOCK_MONOTONIC',
            'batchCount': len(batches), 'frameCount': len(frames),
            'stages': {name: {**percentiles(values), 'population': len(values)} for name, values in stages.items()},
            'limitations': ['Browser data frames/batches are not mapped one-to-one to the 100 physical samples',
                            'First animation opportunity is not compositor acknowledgement or first visible pixel',
                            'ACK spans can include ordered waiting for earlier frames; no isolated IPC RTT claim',
                            'Do not subtract browser timestamps from native timestamps or add stage percentiles']}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--timing', type=Path, required=True)
    parser.add_argument('--native', type=Path, required=True)
    parser.add_argument("--frontend", type=Path, help="Optional browser-local diagnostic report; clock domain stays separate")
    parser.add_argument("--transport", type=Path, help="Optional same-clock Linux native Channel report; requires --frontend")
    parser.add_argument('--output', type=Path, required=True)
    args = parser.parse_args()
    # The two small clock-only reports contain no terminal text or user input.
    blobs = [file.read_bytes() for file in (args.timing, args.native)]
    report = analyze(*(json.loads(blob) for blob in blobs))
    report['inputs'] = [{'file': str(file), 'sha256': hashlib.sha256(blob).hexdigest()}
                        for file, blob in zip((args.timing, args.native), blobs, strict=True)]
    if args.frontend:
        blob = args.frontend.read_bytes()
        report["browserLocal"] = analyze_frontend(json.loads(blob))
        report["inputs"].append({"file": str(args.frontend), "sha256": hashlib.sha256(blob).hexdigest()})
    if args.transport:
        if not args.frontend:
            parser.error('--transport requires --frontend')
        blob = args.transport.read_bytes()
        report['nativeTransport'] = analyze_transport(json.loads(blobs[0]), json.loads(blobs[1]),
                                                     json.loads(args.frontend.read_bytes()), json.loads(blob))
        report['inputs'].append({'file': str(args.transport), 'sha256': hashlib.sha256(blob).hexdigest()})
    with args.output.open('x', encoding='utf-8') as output:
        json.dump(report, output, indent=2, allow_nan=False)
        output.write('\n')
    print(json.dumps({'analysisComplete': True, 'p95Ms': report['stages']['total']['p95Ms'],
                      'targetMet': report['targetMet'], 'output': str(args.output)}))


if __name__ == '__main__':
    main()
