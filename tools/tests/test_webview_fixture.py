"""Test callback deadlines and import safety without a display or GI imports."""
import importlib.util
import io
from contextlib import redirect_stdout, redirect_stderr
import tempfile
from pathlib import Path
import threading
import unittest
from unittest.mock import Mock

PATH = Path(__file__).resolve().parents[2]/'output/tauri-redunar/tests/webview_fixture.py'


def module():
    spec = importlib.util.spec_from_file_location('fixture_under_test', PATH)
    result = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(result)
    return result


class Clock:
    def __init__(self):
        self.events = {}
        self.now = 0
        self.sequence = 0

    def timeout_add(self, milliseconds, callback):
        self.sequence += 1
        self.events[self.sequence] = (self.now+milliseconds, callback)
        return self.sequence

    def source_remove(self, key):
        del self.events[key]

    def MainLoop(self):
        clock = self

        class Loop:
            stopped = False

            def quit(self):
                self.stopped = True

            def run(self):
                while not self.stopped:
                    key = min(clock.events, key=lambda item: clock.events[item][0])
                    clock.now, callback = clock.events.pop(key)
                    callback()

        return Loop()


class Cancellation:
    cancelled = False

    def cancel(self):
        self.cancelled = True


class FixtureTests(unittest.TestCase):
    def fixture(self):
        kind = module().WebviewFixture
        result = kind.__new__(kind)
        result.timeout_ms = 20
        result.GLib = Clock()
        result.Gio = Mock(Cancellable=Cancellation)
        return result

    def test_import_starts_no_resources(self):
        before = set(threading.enumerate())
        loaded = module()
        self.assertEqual(set(threading.enumerate()), before)
        self.assertFalse(hasattr(loaded, 'server'))
        self.assertFalse(hasattr(loaded, 'window'))

    def test_completion_clears_the_deadline_and_returns_its_value(self):
        fixture = self.fixture()
        start = lambda cancel, done: fixture.GLib.timeout_add(5, lambda: done(None, 7, None))
        self.assertEqual(fixture._await(start, lambda view, value: value), 7)
        self.assertEqual(fixture.GLib.events, {})

    def test_timeout_cancels_and_ignores_a_late_callback(self):
        fixture = self.fixture()
        callbacks = []
        finish = Mock()
        with self.assertRaises(TimeoutError):
            fixture._await(lambda cancel, done: callbacks.append((cancel, done)), finish)
        cancel, done = callbacks[0]
        self.assertTrue(cancel.cancelled)
        done(None, 'late', None)
        finish.assert_not_called()

    def test_failed_finish_returns_the_original_exception(self):
        fixture = self.fixture()
        failure = ValueError('snapshot failed')
        with self.assertRaisesRegex(ValueError, 'snapshot failed'):
            fixture._await(lambda cancel, done: done(None, None, None), Mock(side_effect=failure))
        self.assertEqual(fixture.GLib.events, {})

    def test_failed_screenshot_does_not_replace_a_test_failure(self):
        fixture = self.fixture()
        fixture.js = Mock(side_effect=ValueError('original assertion'))
        fixture.snap = Mock(side_effect=TimeoutError('snapshot stalled'))
        with self.assertRaisesRegex(ValueError, 'original assertion'), redirect_stdout(io.StringIO()), redirect_stderr(io.StringIO()):
            fixture.test('assertion', 'throw Error()')

    def test_cleanup_releases_every_owner_even_after_a_webkit_failure(self):
        fixture = self.fixture()
        fixture.closed = False
        fixture.checks = []
        fixture.command_failures = [{'command': 'unknown'}]
        fixture.view = Mock()
        fixture.view.stop_loading.side_effect = RuntimeError('WebKit exited')
        fixture.window = Mock()
        fixture.server = Mock()
        fixture.worker = Mock(ident=1)
        fixture.worker.is_alive.return_value = False
        with tempfile.TemporaryDirectory() as temporary:
            fixture.artifacts = Path(temporary)
            with self.assertRaisesRegex(AssertionError, 'WebKit exited'):
                fixture.close()
            fixture.window.destroy.assert_called_once()
            fixture.server.shutdown.assert_called_once()
            fixture.server.server_close.assert_called_once()
            fixture.worker.join.assert_called_once_with(timeout=2)
            fixture.close()


if __name__ == '__main__':
    unittest.main()
