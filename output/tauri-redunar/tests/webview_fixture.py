"""Explicit, bounded WebKit sessions for synthetic production-UI tests.

Importing this module starts no server, display, window or worker. Each suite
owns a fixture and closes it in a finally block or a with statement.
"""
from functools import partial
from http.server import SimpleHTTPRequestHandler, ThreadingHTTPServer
from pathlib import Path
import json
import os
import sys
import tempfile
import threading
import time

NATIVE = Path(__file__).resolve().parents[1]


class WebviewFixture:
    def __init__(self, suite, timeout_ms=5000):
        self.timeout_ms = timeout_ms
        self.checks = []
        self.command_failures = []
        self.closed = False
        self.server = self.worker = self.window = self.view = None
        name = Path(suite).stem.removesuffix('-webview')
        requested = os.environ.get('REDUNAR_UI_ARTIFACTS')
        if requested:
            self.artifacts = Path(requested)
            self.artifacts.mkdir(parents=True, exist_ok=True)
        else:
            root = NATIVE.parents[1] / '.redunar-build/reports/webview'
            root.mkdir(parents=True, exist_ok=True)
            self.artifacts = Path(tempfile.mkdtemp(prefix=name+'-', dir=root))
        try:
            self._start()
        except BaseException:
            self.close()
            raise

    def _start(self):
        if not (NATIVE / 'ui/dist/index.html').is_file():
            raise FileNotFoundError('Build the production frontend before running this fixture')
        from webkit_fixture import configure_webkit, configure_webview
        configure_webkit()
        import gi
        gi.require_version('Gtk', '3.0')
        gi.require_version('WebKit2', '4.1')
        from gi.repository import GLib, Gio, Gtk, Gdk, GdkPixbuf, WebKit2
        self.gi, self.GLib, self.Gio = gi, GLib, Gio
        self.Gtk, self.Gdk, self.GdkPixbuf, self.WebKit2 = Gtk, Gdk, GdkPixbuf, WebKit2

        # A class per fixture keeps a media handler override local to its suite.
        class Handler(SimpleHTTPRequestHandler):
            def log_message(self, *_):
                pass

        self.Handler = Handler
        self.server = ThreadingHTTPServer(('127.0.0.1', 0), partial(Handler, directory=str(NATIVE/'ui/dist')))
        self.worker = threading.Thread(target=self.server.serve_forever, kwargs={'poll_interval': .05}, daemon=True)
        self.worker.start()
        self.manager = WebKit2.UserContentManager()
        self.manager.register_script_message_handler('fixture')
        self.manager.connect('script-message-received::fixture', self._command_failure)
        palettes = (NATIVE/'ui/overlay-palettes.json').read_text()
        self.add_script('window.fixturePalettes='+palettes+';\n'+(NATIVE/'tests/native-fixture.js').read_text())
        self.view = WebKit2.WebView.new_with_user_content_manager(self.manager)
        configure_webview(self.view)
        self.view.set_background_color(Gdk.RGBA(0, 0, 0, 0))
        self.window = Gtk.Window()
        self.window.set_default_size(1440, 1000)
        self.window.add(self.view)
        self.window.show_all()

    def _command_failure(self, _manager, value):
        self.command_failures.append(json.loads(value.get_js_value().to_string()))

    def add_script(self, source):
        self.manager.add_script(self.WebKit2.UserScript.new(
            source, self.WebKit2.UserContentInjectedFrames.TOP_FRAME,
            self.WebKit2.UserScriptInjectionTime.START, None, None))

    def pump(self, ms=250):
        loop = self.GLib.MainLoop()
        self.GLib.timeout_add(ms, lambda: (loop.quit(), False)[1])
        loop.run()

    def _await(self, start, finish, timeout_ms=None):
        """Bound callbacks as well as polling. Ignore callbacks after cancellation."""
        loop = self.GLib.MainLoop()
        cancel = self.Gio.Cancellable()
        values = []
        complete = False
        expired = False

        def done(view, value, _data):
            nonlocal complete
            if complete:
                return
            complete = True
            try:
                values.append(finish(view, value))
            except BaseException as error:
                values.append(error)
            finally:
                loop.quit()

        def expire():
            nonlocal complete, expired
            complete = expired = True
            cancel.cancel()
            loop.quit()
            return False

        timer = self.GLib.timeout_add(self.timeout_ms if timeout_ms is None else max(1, timeout_ms), expire)
        try:
            start(cancel, done)
            if not complete:
                loop.run()
        except BaseException:
            complete = True
            cancel.cancel()
            raise
        finally:
            if not expired:
                self.GLib.source_remove(timer)
        if expired:
            raise TimeoutError('WebKit callback timed out')
        if isinstance(values[0], BaseException):
            raise values[0]
        return values[0]

    def js(self, source, timeout_ms=None):
        return self._await(
            lambda cancel, done: self.view.evaluate_javascript('(()=>{'+source+'})()', -1, None, None, cancel, done, None),
            lambda view, value: view.evaluate_javascript_finish(value).to_string(), timeout_ms)

    def snap(self, name):
        return self._await(
            lambda cancel, done: self.view.get_snapshot(self.WebKit2.SnapshotRegion.VISIBLE, self.WebKit2.SnapshotOptions.NONE, cancel, done, None),
            lambda view, value: view.get_snapshot_finish(value).write_to_png(str(self.artifacts/(name+'.png'))))

    def route(self, name):
        self.js('location.hash='+json.dumps(name))
        self.pump(400)

    def wait_for(self, expression, timeout_ms=5000):
        deadline = time.monotonic()+timeout_ms/1000
        while time.monotonic() < deadline:
            remaining = max(1, int((deadline-time.monotonic())*1000))
            if self.js('return Boolean('+expression+')', min(self.timeout_ms, remaining)) == 'true':
                return
            self.pump(min(100, remaining))
        raise TimeoutError('Timed out waiting for '+expression)

    def test(self, name, source, wait_ms=0):
        try:
            self.js(source)
        except BaseException:
            try:
                self.snap('failure')
            except BaseException as diagnostic:
                print('Failure screenshot unavailable: '+str(diagnostic), file=sys.stderr)
            print('FAIL: '+name, flush=True)
            raise
        self.checks.append(name)
        if wait_ms:
            self.pump(wait_ms)

    def close(self):
        if self.closed:
            return
        self.closed = True
        failing = sys.exc_info()[0] is not None
        failures = []

        def release(action):
            try:
                action()
            except BaseException as error:
                failures.append(str(error))

        # A failed WebKit operation must not prevent server/window cleanup.
        if self.view is not None:
            release(self.view.stop_loading)
        if self.window is not None:
            release(self.window.destroy)
        if self.server is not None:
            if self.worker is not None and self.worker.ident is not None:
                release(self.server.shutdown)
            release(self.server.server_close)
        if self.worker is not None and self.worker.ident is not None:
            release(lambda: self.worker.join(timeout=2))
            if self.worker.is_alive():
                failures.append('Fixture server worker did not stop')
        if hasattr(self, 'artifacts'):
            (self.artifacts/'fixture-result.json').write_text(json.dumps({
                'status': 'failed' if failing or failures or self.command_failures else 'passed',
                'checks': self.checks, 'unexpected_commands': self.command_failures,
                'cleanup_failures': failures,
            }, indent=2)+'\n')
        if not failing and (failures or self.command_failures):
            raise AssertionError('Fixture cleanup or command failures: '+json.dumps(failures+self.command_failures))

    def __enter__(self):
        return self

    def __exit__(self, *_):
        self.close()
