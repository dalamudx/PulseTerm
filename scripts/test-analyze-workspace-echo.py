#!/usr/bin/env python3
"""Clock correlation and percentile checks; these fixtures are not performance evidence."""
import copy
import importlib.util
from pathlib import Path
import unittest

spec = importlib.util.spec_from_file_location('echo_analysis', Path(__file__).with_name('analyze-workspace-echo.py'))
module = importlib.util.module_from_spec(spec)
spec.loader.exec_module(module)


def fixture():
    timing = {'samplesMs': [], 'startNs': [], 'observedNs': []}
    native = []
    for index in range(100):
        start = 10_000_000_000 + index * 100_000_000
        received = start + 2_000_000 + index * 1000
        written = received + 10_000
        observed = written + 10_000_000 + index * 10_000
        timing['startNs'].append(start)
        timing['observedNs'].append(observed)
        timing['samplesMs'].append((observed - start) / 1_000_000)
        native.append({'receivedNs': received, 'writtenNs': written})
    return timing, native


class AnalysisTests(unittest.TestCase):
    def test_exact_segment_sums_and_nearest_rank_with_all_tail_samples(self):
        result = module.analyze(*fixture())
        self.assertEqual(result['sampleCount'], 100)
        self.assertEqual(result['stages']['total'], {'p50Ms': 12.549, 'p95Ms': 13.044, 'p99Ms': 13.088, 'maxMs': 13.099})
        self.assertEqual(result['stages']['inputToNative']['p95Ms'], 2.094)
        self.assertEqual(result['stages']['nativeHandling']['p95Ms'], .01)
        self.assertEqual(result['stages']['nativeWriteToGlyph']['p95Ms'], 10.94)
        for sample in result['samples']:
            self.assertAlmostEqual(sample['total'], sample['inputToNative'] + sample['nativeHandling'] + sample['nativeWriteToGlyph'])
        self.assertTrue(result['targetMet'])
        self.assertEqual(result['slowSamplesExcluded'], 0)

    def test_stage_percentiles_are_never_added_to_construct_total(self):
        timing, native = fixture()
        for index, start in enumerate(timing['startNs']):
            incoming = 20_000_000 if index % 2 else 1_000_000
            native[index] = {'receivedNs': start + incoming, 'writtenNs': start + incoming}
            timing['observedNs'][index] = start + 21_000_000
            timing['samplesMs'][index] = 21
        result = module.analyze(timing, native)
        self.assertEqual(result['stages']['total']['p95Ms'], 21)
        self.assertEqual(result['stages']['inputToNative']['p95Ms'] + result['stages']['nativeWriteToGlyph']['p95Ms'], 40)
        self.assertFalse(result['targetMet'])

    def test_missing_samples_are_not_truncated_to_a_common_length(self):
        for name in ('samplesMs', 'startNs', 'observedNs', 'native'):
            timing, native = fixture()
            (native if name == 'native' else timing[name]).pop()
            with self.subTest(name=name), self.assertRaises(ValueError):
                module.analyze(timing, native)

    def test_invalid_clock_and_elapsed_data_cannot_be_accepted(self):
        mutations = [
            lambda t, n: t['samplesMs'].__setitem__(0, float('nan')),
            lambda t, n: t['samplesMs'].__setitem__(0, float('inf')),
            lambda t, n: t['samplesMs'].__setitem__(0, True),
            lambda t, n: t['samplesMs'].__setitem__(0, 12),
            lambda t, n: t['startNs'].__setitem__(0, True),
            lambda t, n: n[0].__setitem__('receivedNs', -1),
            lambda t, n: n[0].__setitem__('writtenNs', t['observedNs'][0] + 1),
            lambda t, n: n[0].__setitem__('receivedNs', t['startNs'][0] - 1),
            lambda t, n: t['startNs'].__setitem__(1, t['observedNs'][0]),
            lambda t, n: n.__setitem__(0, None),
        ]
        for index, mutate in enumerate(mutations):
            timing, native = fixture()
            mutate(timing, native)
            with self.subTest(index=index), self.assertRaises(ValueError):
                module.analyze(timing, native)

    def test_input_reports_are_not_mutated(self):
        timing, native = fixture()
        original = copy.deepcopy((timing, native))
        module.analyze(timing, native)
        self.assertEqual((timing, native), original)


def browser_fixture():
    base = 2**60 + 7
    return {'schemaVersion': 1, 'overflow': False, 'batches': [{
        'id': 1, 'offset': str(base), 'end': str(base + 4), 'startedMs': 10, 'parsedMs': 11,
        'preparedMs': 12, 'firstFrameMs': 13, 'lastFrameMs': 14, 'frameCount': 2,
        'fencedMs': 15, 'verifiedMs': 16}], 'frames': [
        {'sequence': 8, 'offset': str(base), 'end': str(base + 2), 'sourceBytes': '100',
         'batch': 1, 'arrivedMs': 4, 'acknowledgedMs': 17},
        {'sequence': 9, 'offset': str(base + 2), 'end': str(base + 4), 'sourceBytes': '100',
         'batch': 1, 'arrivedMs': 5, 'acknowledgedMs': 18}]}


class BrowserAnalysisTests(unittest.TestCase):
    def test_local_populations_and_stages_include_merged_frames(self):
        result = module.analyze_frontend(browser_fixture())
        self.assertEqual((result['batchCount'], result['frameCount']), (1, 2))
        stages = result['stages']
        self.assertEqual(stages['parser']['population'], 1)
        self.assertEqual(stages['queueToParser']['population'], 2)
        self.assertEqual(stages['queueToParser']['p95Ms'], 6)
        self.assertEqual(stages['firstAnimationWait']['p95Ms'], 1)
        self.assertEqual(stages['remainingFence']['p95Ms'], 2)
        self.assertEqual(stages['verifiedToAckCompletion']['p95Ms'], 2)
        self.assertIn('not aligned', result['clockDomain'])

    def test_absolute_browser_clock_shift_cannot_change_local_durations(self):
        report = browser_fixture()
        expected = module.analyze_frontend(report)['stages']
        for row in report['batches'] + report['frames']:
            for key in row:
                if key.endswith('Ms'):
                    row[key] += 100_000
        self.assertEqual(module.analyze_frontend(report)['stages'], expected)

    def test_incomplete_or_unbounded_metadata_is_rejected(self):
        for key, value in [('overflow', True), ('schemaVersion', 2), ('batches', []), ('frames', [])]:
            report = browser_fixture()
            report[key] = value
            with self.subTest(key=key), self.assertRaises(ValueError):
                module.analyze_frontend(report)
        report = browser_fixture()
        report['frames'] *= 129
        with self.assertRaises(ValueError):
            module.analyze_frontend(report)

    def test_invalid_times_coordinates_ids_or_partial_coverage_reject(self):
        variants = [
            ('batches', 0, 'firstFrameMs', 9), ('batches', 0, 'frameCount', 1),
            ('batches', 0, 'verifiedMs', float('nan')), ('batches', 0, 'parsedMs', True),
            ('frames', 0, 'offset', '01'), ('frames', 0, 'sourceBytes', str(2**64)),
            ('frames', 1, 'sequence', 8), ('frames', 0, 'batch', 2),
            ('frames', 0, 'arrivedMs', 11), ('frames', 0, 'acknowledgedMs', 15),
            ('frames', 1, 'end', str(2**60 + 10)),
        ]
        for section, index, key, value in variants:
            report = browser_fixture()
            report[section][index][key] = value
            with self.subTest(key=key), self.assertRaises(ValueError):
                module.analyze_frontend(report)


def transport_fixture():
    timing, native = fixture()
    browser = {'schemaVersion': 1, 'overflow': False, 'batches': [], 'frames': []}
    transport = {'schemaVersion': 1, 'clockDomain': 'Linux CLOCK_MONOTONIC', 'generation': '9', 'frames': []}
    for index, sample in enumerate(native):
        sample['writeStartedNs'] = sample['receivedNs'] + 5000
        base = 2**60 + 7 + index * 4
        elapsed = index * 100
        browser['batches'].append({'id': index + 1, 'offset': str(base), 'end': str(base + 4),
            'startedMs': elapsed + 10, 'parsedMs': elapsed + 11, 'preparedMs': elapsed + 12,
            'firstFrameMs': elapsed + 13, 'lastFrameMs': elapsed + 14, 'frameCount': 2,
            'fencedMs': elapsed + 15, 'verifiedMs': elapsed + 16})
        browser['frames'].append({'sequence': index + 8, 'offset': str(base), 'end': str(base + 4),
            'sourceBytes': str(1000 + index * 4), 'batch': index + 1,
            'arrivedMs': elapsed + 5, 'acknowledgeStartedMs': elapsed + 17, 'acknowledgedMs': elapsed + 18})
        sent = sample['writtenNs'] + 1_000_000
        transport['frames'].append({'sequence': index + 8, 'offset': str(base), 'end': str(base + 4),
            'sourceBytes': str(1000 + index * 4), 'sentNs': sent,
            'sendReturnedNs': sent + 20_000, 'acknowledgedNs': sent + 30_000_000})
    return timing, native, browser, transport


class NativeTransportAnalysisTests(unittest.TestCase):
    def test_all_100_samples_match_exact_frame_identity_and_same_clock(self):
        reports = transport_fixture()
        original = copy.deepcopy(reports)
        result = module.analyze_transport(*reports)
        self.assertEqual(result['sampleCount'], 100)
        self.assertTrue(result['browserFrameIdentityMatched'])
        self.assertAlmostEqual(result['stages']['nativeWriteStartToChannel']['p95Ms'], 1.005)
        self.assertEqual(result['stages']['channelSubmissionCall']['p95Ms'], .02)
        self.assertEqual(reports, original)

    def test_browser_absolute_clock_shift_does_not_change_native_stages(self):
        reports = transport_fixture()
        expected = module.analyze_transport(*reports)['stages']
        for row in reports[2]['batches'] + reports[2]['frames']:
            for key in row:
                if key.endswith('Ms'):
                    row[key] += 100_000
        self.assertEqual(module.analyze_transport(*reports)['stages'], expected)

    def test_wrong_coordinates_missing_ack_backwards_or_invalid_native_clock_reject(self):
        for key, value in [('offset', '01'), ('sourceBytes', '999'), ('sequence', 10),
                           ('acknowledgedNs', None), ('sentNs', True), ('sendReturnedNs', 1)]:
            reports = transport_fixture()
            reports[3]['frames'][0][key] = value
            with self.subTest(key=key), self.assertRaises(ValueError):
                module.analyze_transport(*reports)
        reports = transport_fixture()
        reports[3]['clockDomain'] = 'browser performance.now'
        with self.assertRaises(ValueError):
            module.analyze_transport(*reports)

    def test_ambiguous_missing_or_reused_samples_fail_without_truncation(self):
        reports = transport_fixture()
        reports[3]['frames'][1].update(sentNs=reports[3]['frames'][0]['sentNs'] + 1)
        with self.assertRaises(ValueError):
            module.analyze_transport(*reports)
        reports = transport_fixture()
        reports[3]['frames'][0]['sentNs'] = reports[0]['observedNs'][0] + 1
        reports[3]['frames'][0]['sendReturnedNs'] = reports[3]['frames'][0]['sentNs'] + 1
        with self.assertRaises(ValueError):
            module.analyze_transport(*reports)
        reports = transport_fixture()
        reports[3]['frames'].pop()
        with self.assertRaises(ValueError):
            module.analyze_transport(*reports)

    def test_write_start_clock_is_required_instead_of_inventing_absolute_alignment(self):
        reports = transport_fixture()
        del reports[1][0]['writeStartedNs']
        with self.assertRaises(ValueError):
            module.analyze_transport(*reports)
        reports = transport_fixture()
        reports[1][0]['writeStartedNs'] = reports[1][0]['writtenNs'] + 1
        with self.assertRaises(ValueError):
            module.analyze_transport(*reports)


def delivery_fixture():
    timing, native, browser, transport = transport_fixture()
    anchor = transport['frames'][0]['sentNs'] - 100_000_000
    for index, (frame, rendered, batch) in enumerate(zip(transport['frames'], browser['frames'], browser['batches'], strict=True)):
        sent = frame['sentNs']
        local = (sent - anchor) / 1_000_000
        rendered.update(arrivedMs=local + 2, acknowledgeStartedMs=local + 24, acknowledgedMs=local + 29)
        batch.update(startedMs=local + 2.2, parsedMs=local + 3, preparedMs=local + 4,
                     firstFrameMs=local + 10, lastFrameMs=local + 20, fencedMs=local + 20.5, verifiedMs=local + 22)
        frame['acknowledgedNs'] = sent + 27_000_000
        timing['observedNs'][index] = sent + 40_000_000
        timing['samplesMs'][index] = (timing['observedNs'][index] - timing['startNs'][index]) / 1_000_000
    return timing, native, browser, transport


class DeliveryBoundTests(unittest.TestCase):
    def test_bounds_contain_known_forward_and_post_verify_delays(self):
        result = module.analyze_transport(*delivery_fixture())['deliveryBounds']
        self.assertTrue(result['available'])
        self.assertFalse(result['assumptions']['independentlyCalibrated'])
        self.assertEqual(result['postVerificationDelayEstablishedSamplesUnderAssumptions'], 100)
        for row in result['samples']:
            self.assertLessEqual(row['channelDeliveryLowerMs'], 2)
            self.assertGreaterEqual(row['channelDeliveryUpperMs'], 2)
            self.assertLessEqual(row['verifiedToGlyphLowerMs'], 18)
            self.assertGreaterEqual(row['verifiedToGlyphUpperMs'], 18)

    def test_absolute_browser_shift_cannot_change_causal_bounds(self):
        reports = delivery_fixture()
        expected = module.analyze_transport(*reports)['deliveryBounds']['stages']
        for row in reports[2]['batches'] + reports[2]['frames']:
            for key in row:
                if key.endswith('Ms'):
                    row[key] += 100_000
        actual = module.analyze_transport(*reports)['deliveryBounds']['stages']
        for name in actual:
            for percentile in actual[name]:
                self.assertAlmostEqual(actual[name][percentile], expected[name][percentile], places=7)

    def test_legacy_report_has_no_invented_bounds_and_partial_metadata_fails(self):
        reports = delivery_fixture()
        for frame in reports[2]['frames']:
            del frame['acknowledgeStartedMs']
        self.assertFalse(module.analyze_transport(*reports)['deliveryBounds']['available'])
        reports[2]['frames'][0]['acknowledgeStartedMs'] = reports[2]['frames'][0]['acknowledgedMs']
        with self.assertRaises(ValueError):
            module.analyze_transport(*reports)

    def test_invalid_ack_start_and_contradictory_causality_reject(self):
        for value in (-1, True, float('nan'), 1_000_000):
            reports = delivery_fixture()
            reports[2]['frames'][0]['acknowledgeStartedMs'] = value
            with self.subTest(value=value), self.assertRaises(ValueError):
                module.analyze_transport(*reports)
        reports = delivery_fixture()
        reports[3]['frames'][0]['acknowledgedNs'] = reports[3]['frames'][0]['sentNs'] + 5_000_000
        with self.assertRaisesRegex(ValueError, 'contradicts'):
            module.analyze_transport(*reports)


class RenderSchedulingIdentityTests(unittest.TestCase):
    def test_each_correlated_sample_retains_actual_render_mode(self):
        reports = delivery_fixture()
        for index, batch in enumerate(reports[2]['batches']):
            batch['postFrameTask'] = index % 2 == 1
        result = module.analyze_transport(*reports)
        self.assertEqual([row['postFrameTask'] for row in result['samples']], [i % 2 == 1 for i in range(100)])

    def test_invalid_mode_rejects_and_historical_modes_are_not_invented(self):
        reports = delivery_fixture()
        self.assertTrue(all(row['postFrameTask'] is None for row in module.analyze_transport(*reports)['samples']))
        for value in (1, 'true', None):
            reports[2]['batches'][0]['postFrameTask'] = value
            with self.subTest(value=value), self.assertRaises(ValueError):
                module.analyze_transport(*reports)


class InputPacingTests(unittest.TestCase):
    def test_historical_schedule_stays_unknown(self):
        self.assertFalse(module.analyze(*fixture())['pacing']['available'])

    def test_valid_fixed_and_variable_schedules_do_not_change_latency_samples(self):
        for mode, pauses in [('fixed', [20_000_000] * 100), ('jitter', [5_000_000, 35_000_000] * 50)]:
            timing, native = fixture()
            original = module.analyze(timing, native)
            timing.update(pacingMode=mode, requestedPauseNs=pauses)
            result = module.analyze(timing, native)
            self.assertEqual(result['stages'], original['stages'])
            self.assertEqual(result['pacing']['mode'], mode)
            self.assertEqual(result['pacing']['requestedPauseMeanMs'], 20)

    def test_incomplete_or_invalid_pacing_never_becomes_a_valid_comparison(self):
        for mode, pauses in [('fixed', []), ('fixed', [20_000_000] * 99), ('fixed', [19_000_000] * 100),
                             ('jitter', [20_000_000] * 100), ('jitter', [5_000_000, 34_000_000] * 50),
                             ('jitter', [4_000_000, 36_000_000] * 50), ('jitter', [True] * 100),
                             ('unknown', [20_000_000] * 100)]:
            timing, native = fixture()
            timing.update(pacingMode=mode, requestedPauseNs=pauses)
            with self.subTest(mode=mode, pauses=pauses[:2]), self.assertRaises(ValueError):
                module.analyze(timing, native)
        timing, native = fixture()
        timing['pacingMode'] = 'fixed'
        with self.assertRaises(ValueError):
            module.analyze(timing, native)


if __name__ == '__main__':
    unittest.main()
